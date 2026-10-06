use super::*;
use std::time::Instant;
const PROCESS_SCRIPT: &str = r#"export LC_ALL=C
printf 'META '; getconf CLK_TCK
printf 'PAGE '; getconf PAGESIZE
awk '/^MemTotal:/ {print "MEM " $2}' /proc/meminfo
probe_count=0
for probe_stat in /proc/[0-9]*/stat; do
  [ "$probe_count" -lt 4096 ] || break
  IFS= read -r probe_line < "$probe_stat" 2>/dev/null || continue
  printf '%s\n' "$probe_line"
  probe_count=$((probe_count + 1))
done
"#;
#[derive(Default)]
pub(super) struct Inspector {
    previous: HashMap<(u32, u64), u64>,
    sampled: Option<Instant>,
    system: Option<(Instant, Value)>,
}
pub(super) fn action(s: &str) -> bool {
    matches!(s, "processes" | "services" | "service_logs" | "system")
}
fn valid_unit(unit: &str) -> bool {
    !unit.starts_with('-')
        && unit.ends_with(".service")
        && unit.len() <= 200
        && unit
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._@:-".contains(&b))
}
#[derive(Debug)]
struct Process {
    pid: u32,
    start: u64,
    ticks: u64,
    rss: u64,
    name: String,
}
fn process(line: &str, page: u64) -> Option<Process> {
    let (pid, tail) = line.split_once(" (")?;
    let end = tail.rfind(") ")?;
    let name = &tail[..end];
    let fields: Vec<_> = tail[end + 2..].split_ascii_whitespace().collect();
    if fields.len() < 22 || name.chars().any(char::is_control) {
        return None;
    }
    let rss = fields[21].parse::<i64>().ok()?.max(0) as u64;
    Some(Process {
        pid: pid.parse().ok()?,
        start: fields[19].parse().ok()?,
        ticks: fields[11]
            .parse::<u64>()
            .ok()?
            .saturating_add(fields[12].parse().ok()?),
        rss: rss.saturating_mul(page),
        name: name.chars().take(100).collect(),
    })
}
impl Inspector {
    pub(super) async fn handle(
        &mut self,
        client: &ssh::Client,
        r: &Request,
    ) -> Result<Value, Problem> {
        if r.action == "system" {
            if r.path != "/" || !r.target.is_empty() || !r.content.is_empty() || r.offset != 0 {
                return Err(error("invalid", "监测请求参数不正确"));
            }
            if let Some((at, data)) = &self.system
                && at.elapsed() < Duration::from_secs(10)
            {
                return Ok(data.clone());
            }
            let raw = ssh::exec(client, "/bin/sh -s", SYSTEM_SCRIPT.as_bytes(), 32768)
                .await
                .map_err(|_| error("unavailable", "系统监测暂不可用"))?;
            let data = system_result(&raw);
            self.system = Some((Instant::now(), data.clone()));
            return Ok(data);
        }
        if r.action == "processes" {
            if !["", "cpu", "memory"].contains(&r.target.as_str()) {
                return Err(error("invalid", "进程排序方式不正确"));
            }
            if self
                .sampled
                .is_some_and(|t| t.elapsed() < Duration::from_millis(900))
            {
                return Err(error("rate", "请稍后刷新进程信息"));
            }
            let raw = ssh::exec(client, "/bin/sh -s", PROCESS_SCRIPT.as_bytes(), 1024 * 1024)
                .await
                .map_err(|_| error("unavailable", "无法读取进程信息，请检查当前 SSH 用户权限"))?;
            let mut hz = 100.;
            let mut page = 4096;
            let mut memory = 0u64;
            let mut rows = Vec::new();
            let mut next = HashMap::new();
            let at = Instant::now();
            let elapsed = self.sampled.map(|t| at.duration_since(t).as_secs_f64());
            for line in raw.lines() {
                if let Some(v) = line.strip_prefix("META ") {
                    hz = v
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| *v > 0. && *v < 100000.)
                        .unwrap_or(100.);
                    continue;
                }
                if let Some(v) = line.strip_prefix("PAGE ") {
                    page = v
                        .trim()
                        .parse::<u64>()
                        .ok()
                        .filter(|v| *v > 0 && *v <= 65536)
                        .unwrap_or(4096);
                    continue;
                }
                if let Some(v) = line.strip_prefix("MEM ") {
                    memory = v.trim().parse::<u64>().unwrap_or(0).saturating_mul(1024);
                    continue;
                }
                if let Some(p) = process(line, page) {
                    if next.len() >= 4096 {
                        break;
                    }
                    let cpu = elapsed.and_then(|seconds| {
                        self.previous
                            .get(&(p.pid, p.start))
                            .filter(|ticks| p.ticks >= **ticks)
                            .map(|ticks| {
                                ((p.ticks - *ticks) as f64 / hz / seconds * 100.).min(100000.)
                            })
                    });
                    next.insert((p.pid, p.start), p.ticks);
                    rows.push(json!({"pid":p.pid,"name":p.name,"cpu":cpu,"memory":p.rss,"memoryPercent":if memory>0 {p.rss as f64/memory as f64*100.}else{0.}}));
                }
            }
            self.previous = next;
            self.sampled = Some(at);
            rows.sort_by(|a, b| {
                if r.target == "memory" {
                    b["memory"].as_u64().cmp(&a["memory"].as_u64())
                } else {
                    b["cpu"]
                        .as_f64()
                        .unwrap_or(0.)
                        .total_cmp(&a["cpu"].as_f64().unwrap_or(0.))
                        .then(b["memory"].as_u64().cmp(&a["memory"].as_u64()))
                }
            });
            let total = rows.len();
            rows.truncate(100);
            return Ok(
                json!({"processes":rows,"sampled":elapsed.is_some(),"total":total,"at":now(),"limited":total>=4096}),
            );
        }
        if r.action == "services" {
            let raw=ssh::exec(client,"LC_ALL=C SYSTEMD_COLORS=0 /usr/bin/systemctl list-units --type=service --all --no-legend --no-pager --plain",&[],256*1024).await
                .map_err(|_|error("unavailable","无法读取 systemd 服务，请检查系统支持与 SSH 用户权限"))?;
            let rows:Vec<_>=raw.lines().filter_map(|line|{
                let mut fields=line.split_whitespace();
                let unit=fields.next()?;if !valid_unit(unit){return None;}
                Some(json!({"unit":unit,"load":fields.next()?,"active":fields.next()?,"state":fields.next()?,"description":fields.collect::<Vec<_>>().join(" ").chars().take(200).collect::<String>()}))
            }).take(1000).collect();
            return Ok(json!({"services":rows,"at":now()}));
        }
        if r.action == "service_logs" {
            if !valid_unit(&r.target) {
                return Err(error("invalid", "服务名称不正确"));
            }
            let cmd = format!(
                "LC_ALL=C SYSTEMD_COLORS=0 /usr/bin/journalctl --no-pager --quiet --output=short-iso --lines=100 --unit={}",
                ssh::quote(&r.target)
            );
            let raw = ssh::exec(client, &cmd, &[], 256 * 1024)
                .await
                .map_err(|_| {
                    error(
                        "unavailable",
                        "无法读取日志，可能缺少 journal 权限或日志超过限制",
                    )
                })?;
            return Ok(json!({"unit":r.target,"text":raw,"at":now()}));
        }
        Err(error("invalid", "不支持的运行状态请求"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_names_reject_shell_and_option_injection() {
        for s in ["nginx.service", "ssh@22.service", "docker.service"] {
            assert!(valid_unit(s));
        }
        for s in [
            "--all.service",
            "ssh.service;id",
            "$(id).service",
            "a\n.service",
            "a/../b.service",
        ] {
            assert!(!valid_unit(s));
        }
    }
    #[test]
    fn proc_stat_handles_names_with_spaces_and_parentheses() {
        let mut fields = vec!["0"; 30];
        fields[0] = "S";
        fields[11] = "120";
        fields[12] = "20";
        fields[19] = "100";
        fields[21] = "5";
        let line = format!("42 (test ) name) {}", fields.join(" "));
        let p = process(&line, 4096).unwrap();
        assert_eq!((p.pid, p.start, p.ticks, p.rss), (42, 100, 140, 20480));
        assert_eq!(p.name, "test ) name");
    }
}

// Read filesystem counters only: no recursive directory walk and no user input.
// A local-filesystem filter avoids network mounts; timeout bounds a stuck disk.
const SYSTEM_SCRIPT: &str = r#"export LC_ALL=C
printf 'LOAD '; cat /proc/loadavg 2>/dev/null
printf '\n'
if command -v timeout >/dev/null 2>&1 && command -v df >/dev/null 2>&1; then
  timeout -s TERM -k 1 3s df -Pk -l -x tmpfs -x devtmpfs -x squashfs 2>/dev/null
fi
exit 0
"#;
fn filesystem(line: &str) -> Option<Value> {
    // Locate the percentage field; the device and mount path can contain spaces.
    for (i, _) in line.match_indices('%') {
        let prefix = &line[..i];
        let mut fields = prefix.split_ascii_whitespace().rev();
        let Some(percent) = fields.next().and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        if percent > 1000 {
            continue;
        }
        let counts: Option<Vec<u64>> = fields
            .by_ref()
            .take(3)
            .map(|v| v.parse::<u64>().ok()?.checked_mul(1024))
            .collect();
        let Some(counts) = counts.filter(|v| v.len() == 3 && v[2] > 0) else {
            continue;
        };
        if fields.next().is_none() {
            continue;
        }
        let tail = &line[i + 1..];
        if !tail.starts_with([' ', '\t']) {
            continue;
        }
        let path = tail.trim_start_matches([' ', '\t']);
        if !path.starts_with('/') || path.len() > 4096 || path.chars().any(char::is_control) {
            continue;
        }
        let total = counts[2];
        return Some(json!({"path":path,"total":total,
            "available":counts[0].min(total),"used":counts[1].min(total)}));
    }
    None
}
fn system_result(raw: &str) -> Value {
    let load: Vec<f64> = raw
        .lines()
        .find_map(|line| line.strip_prefix("LOAD "))
        .unwrap_or("")
        .split_ascii_whitespace()
        .take(3)
        .filter_map(|s| s.parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.))
        .collect();
    let mut paths = std::collections::HashSet::new();
    let volumes: Vec<_> = raw
        .lines()
        .take(512)
        .filter_map(filesystem)
        .filter(|v| paths.insert(v["path"].as_str().unwrap_or("").to_owned()))
        .take(128)
        .collect();
    json!({"load":if load.len()==3 {json!(load)}else{json!([])},
        "filesystems":volumes,"at":now()})
}
#[cfg(test)]
mod filesystem_tests {
    use super::*;
    #[test]
    fn available_is_not_total_minus_used_and_mount_spaces_are_preserved() {
        let value = system_result(
            "LOAD 0.10 1.25 2.40 1/42 123\nFilesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda 100 30 65 30% /\n/dev/path with spaces 200 90 100 45% /mnt/a b% c\n/dev/sda 100 30 65 30% /\n",
        );
        assert_eq!(value["load"], json!([0.1, 1.25, 2.4]));
        assert_eq!(value["filesystems"].as_array().unwrap().len(), 2);
        assert_eq!(value["filesystems"][0]["available"], 65 * 1024);
        assert_eq!(value["filesystems"][0]["total"], 100 * 1024);
        assert_eq!(value["filesystems"][1]["path"], "/mnt/a b% c");
    }
    #[test]
    fn reject_overflow_control_bytes_malformed_counters_and_unavailable_data() {
        for line in [
            "/dev/sda 1 1 1 1% relative",
            "/dev/sda 1 1 1 1% /bad\u{1b}name",
            "/dev/sda 18446744073709551615 1 1 1% /",
            "/dev/sda 1 -1 1 1% /",
            "/dev/sda 0 0 0 0% /",
            "/dev/sda 1 1 1 1%/malformed",
        ] {
            assert!(filesystem(line).is_none(), "{line}");
        }
        let data = system_result("LOAD NaN inf -1\n");
        assert_eq!(data["load"], json!([]));
        assert_eq!(data["filesystems"], json!([]));
        assert!(!action("root_usage"));
    }
}
