use crate::{core::*, model::Auth};
use chrono::DateTime;
use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use serde_json::json;
use sha1::Sha1;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Login {
    username: String,
    password: String,
    code: String,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Password {
    current: String,
    new: String,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UsernameInput {
    username: String,
    current: String,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Mfa {
    password: String,
    code: String,
}
pub fn totp(secret: &str, counter: i64) -> Option<String> {
    let key = BASE32_NOPAD.decode(secret.as_bytes()).ok()?;
    if key.len() != 20 || counter < 0 {
        return None;
    }
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(&key).ok()?;
    mac.update(&(counter as u64).to_be_bytes());
    let sum = mac.finalize().into_bytes();
    let off = usize::from(sum[19] & 15);
    let number = u32::from_be_bytes(sum[off..off + 4].try_into().ok()?) & 0x7fffffff;
    Some(format!("{:06}", number % 1_000_000))
}
pub fn verify_totp(secret: &str, code: &str, at: i64, last: i64) -> Option<i64> {
    if code.len() != 6 || !code.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    [at / 30, at / 30 - 1, at / 30 + 1]
        .into_iter()
        .find(|&step| step > last && totp(secret, step).is_some_and(|value| constant(&value, code)))
}
pub(crate) fn reserve(
    app: &App,
    i: &mut Inner,
    key: &str,
    limit: u32,
    global: bool,
) -> ApiResult<()> {
    let t = now();
    i.limits.sources.retain(|_, a| a.until.timestamp() > t);
    if i.limits.sources.len() > 8192 {
        return Err(ApiError::new(503, "服务繁忙"));
    }
    let a = i.limits.sources.entry(key.into()).or_default();
    if a.until.timestamp() <= t {
        *a = Attempt {
            count: 0,
            until: DateTime::from_timestamp(t + 900, 0).unwrap(),
        };
    }
    if global && i.limits.global.until.timestamp() <= t {
        i.limits.global = Attempt {
            count: 0,
            until: DateTime::from_timestamp(t + 900, 0).unwrap(),
        };
    }
    if a.count >= limit || (global && i.limits.global.count >= 24) {
        return Err(ApiError::rate("验证次数过多，请 15 分钟后重试", 900));
    }
    a.count += 1;
    if global {
        i.limits.global.count += 1;
    }
    app.persist_limits(i)
}
pub(crate) async fn password_matches(hash: String, password: String) -> bool {
    if password.len() > 72 {
        return false;
    }
    tokio::task::spawn_blocking(move || bcrypt::verify(password, &hash).unwrap_or(false))
        .await
        .unwrap_or(false)
}
pub(crate) fn save_auth(app: &App, i: &mut Inner, auth: Auth) -> ApiResult<()> {
    atomic_json(&app.0.dir.join("auth.json"), &auth).map_err(|_| ApiError::internal())?;
    i.auth = auth;
    Ok(())
}
fn login_mfa(app: &App, i: &mut Inner, code: &str) -> bool {
    if i.auth.mfa.is_empty() {
        return true;
    }
    if let Some(digest) = recovery_digest(code) {
        if let Some(index) = i.auth.recovery.iter().position(|v| constant(v, &digest)) {
            let mut auth = i.auth.clone();
            auth.recovery.remove(index);
            auth.version = token();
            if save_auth(app, i, auth).is_err() {
                return false;
            }
            let ids: Vec<_> = i
                .sessions
                .values()
                .filter(|s| s.auth)
                .map(|s| s.id.clone())
                .collect();
            for id in ids {
                i.revoke(&id);
            }
            app.record(i, "recovery_used", "一次性恢复码");
            return true;
        }
        return false;
    }
    let Ok(raw) = app.unseal("admin:mfa", &i.auth.mfa) else {
        return false;
    };
    let Ok(secret) = std::str::from_utf8(&raw) else {
        return false;
    };
    let Some(counter) = verify_totp(secret, code, now(), i.auth.mfa_last) else {
        return false;
    };
    let mut auth = i.auth.clone();
    auth.mfa_last = counter;
    save_auth(app, i, auth).is_ok()
}
pub(crate) fn invalidate(i: &mut Inner) {
    let ids: Vec<_> = i.sessions.keys().cloned().collect();
    for id in ids {
        i.revoke(&id);
    }
}
pub async fn login(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    let mut v: Login = decode(&body)?;
    let _slot = app
        .0
        .login_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::rate("请稍后再试", 1))?;
    let (x, hash, version) = {
        let mut i = app.lock();
        let x = app.guard(&mut i, &c, false, true)?;
        reserve(&app, &mut i, &c.ip, 8, true)?;
        (x, i.auth.hash.clone(), i.auth.version.clone())
    };
    if v.password.len() > 72 || v.username.len() > 80 {
        v.password = "invalid".into();
    }
    let matched = password_matches(hash, v.password).await;
    let mut i = app.lock();
    if !matched
        || v.username != i.auth.username
        || version != i.auth.version
        || !login_mfa(&app, &mut i, &v.code)
    {
        app.record(&mut i, "login_failed", &c.ip);
        return Err(ApiError::new(401, "用户名、密码或验证码不正确"));
    }
    if !i.session(&c.sid).is_some_and(|s| s.id == x.id) {
        return Err(ApiError::new(401, "登录页面已过期，请刷新"));
    }
    i.limits.sources.remove(&c.ip);
    i.limits.global = Attempt::default();
    app.persist_limits(&mut i)?;
    let x = i.new_session(&x.id, true)?;
    if let Some(s) = i.sessions.get_mut(&x.id) {
        s.source = c.ip.clone();
        s.device = c.header("User-Agent").chars().take(200).collect();
        s.elevated = now();
    }
    let name = i.auth.username.clone();
    app.record(&mut i, "login", &name);
    if let Some(a) = i.audit.last_mut() {
        a.source = c.ip.clone();
    }
    crate::operations::notify_login(&app, &mut i, &c.ip);
    Ok(ApiReply::session(i.info(&x), &x.id))
}
pub async fn password(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    let v: Password = decode(&body)?;
    if !valid_password(&v.new) {
        return Err(ApiError::new(400, "新密码需为 12–72 字节"));
    }
    let _slot = app
        .0
        .login_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::rate("请稍后再试", 1))?;
    let (x, hash, version) = {
        let mut i = app.lock();
        let x = app.guard(&mut i, &c, true, true)?;
        reserve(&app, &mut i, "password:account", 5, false)?;
        (x, i.auth.hash.clone(), i.auth.version.clone())
    };
    if !password_matches(hash, v.current).await {
        return Err(ApiError::new(400, "当前密码不正确"));
    }
    let hash = tokio::task::spawn_blocking(move || bcrypt::hash(v.new, 12))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|_| ApiError::internal())?;
    let mut i = app.lock();
    app.guard(&mut i, &c, true, true)?;
    if i.auth.version != version {
        return Err(ApiError::new(409, "账户设置已变化，请重新登录"));
    }
    let mut auth = i.auth.clone();
    auth.hash = hash;
    auth.version = token();
    save_auth(&app, &mut i, auth)?;
    invalidate(&mut i);
    let x = i.new_session(&x.id, true)?;
    let name = i.auth.username.clone();
    app.record(&mut i, "password_changed", &name);
    Ok(ApiReply::session(i.info(&x), &x.id))
}
pub async fn change_username(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    let mut v: UsernameInput = decode(&body)?;
    v.username = v.username.trim().to_owned();
    if !username(&v.username) {
        return Err(ApiError::new(
            400,
            "用户名需为 1–32 位，以字母或下划线开头，可含字母、数字、下划线、点和连字符",
        ));
    }
    let _slot = app
        .0
        .login_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::rate("请稍后再试", 1))?;
    let (x, hash, version) = {
        let mut i = app.lock();
        let x = app.guard(&mut i, &c, true, true)?;
        if v.username == i.auth.username {
            return Err(ApiError::new(400, "新用户名与当前用户名相同"));
        }
        // Share the password-change limit; adding this action must not create
        // another independent budget for guessing the current password.
        reserve(&app, &mut i, "password:account", 5, false)?;
        (x, i.auth.hash.clone(), i.auth.version.clone())
    };
    if !password_matches(hash, v.current).await {
        return Err(ApiError::new(400, "当前密码不正确"));
    }
    let mut i = app.lock();
    app.guard(&mut i, &c, true, true)?;
    if i.auth.version != version {
        return Err(ApiError::new(409, "账户设置已变化，请重新登录"));
    }
    let old = i.auth.username.clone();
    let mut auth = i.auth.clone();
    auth.username = v.username;
    auth.version = token();
    save_auth(&app, &mut i, auth)?;
    invalidate(&mut i);
    let next = i.new_session(&x.id, true)?;
    if let Some(session) = i.sessions.get_mut(&next.id) {
        session.source = x.source;
        session.device = x.device;
    }
    let subject = format!("{} → {}", old, i.auth.username);
    app.record(&mut i, "username_changed", &subject);
    Ok(ApiReply::session(i.info(&next), &next.id))
}
pub async fn mfa(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    if c.method != "POST" {
        return Err(ApiError::new(405, "请求方法不正确"));
    }
    let v: Mfa = decode(&body)?;
    let action = c.path.strip_prefix("/api/admin/mfa/").unwrap_or("");
    if !["setup", "enable", "disable"].contains(&action) {
        return Err(ApiError::new(404, "接口不存在"));
    }
    let _slot = app
        .0
        .login_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::rate("请稍后再试", 1))?;
    let (x, hash, version) = {
        let mut i = app.lock();
        let x = app.guard(&mut i, &c, true, true)?;
        reserve(&app, &mut i, "mfa:account", 5, false)?;
        (x, i.auth.hash.clone(), i.auth.version.clone())
    };
    if action != "enable" && !password_matches(hash, v.password).await {
        return Err(ApiError::new(400, "当前密码或验证码不正确"));
    }
    let mut i = app.lock();
    app.guard(&mut i, &c, true, true)?;
    if i.auth.version != version {
        return Err(ApiError::new(409, "账户设置已变化，请重新登录"));
    }
    if action == "setup" {
        if !i.auth.mfa.is_empty() {
            return Err(ApiError::new(409, "二步验证已经启用"));
        }
        let mut bytes = [0; 20];
        getrandom::fill(&mut bytes).map_err(|_| ApiError::internal())?;
        let secret = BASE32_NOPAD.encode(&bytes);
        let s = i
            .sessions
            .get_mut(&x.id)
            .ok_or_else(|| ApiError::new(401, "登录已过期"))?;
        s.mfa_pending = secret.clone();
        s.mfa_expires = now() + 300;
        return Ok(ApiReply::ok(json!({"secret":secret})));
    }
    let mut auth = i.auth.clone();
    let mut recovery = Vec::new();
    if action == "enable" {
        if !auth.mfa.is_empty() || x.mfa_pending.is_empty() || now() > x.mfa_expires {
            return Err(ApiError::new(400, "绑定已过期，请重新开始"));
        }
        let counter = verify_totp(&x.mfa_pending, &v.code, now(), 0)
            .ok_or_else(|| ApiError::new(400, "验证码不正确，请检查验证器时间"))?;
        auth.mfa = app
            .seal("admin:mfa", x.mfa_pending.as_bytes())
            .map_err(|_| ApiError::internal())?;
        auth.mfa_last = counter;
        recovery = new_recovery(&mut auth);
    } else {
        if auth.mfa.is_empty() {
            return Err(ApiError::new(409, "二步验证尚未启用"));
        }
        let raw = app
            .unseal("admin:mfa", &auth.mfa)
            .map_err(|_| ApiError::internal())?;
        let secret = std::str::from_utf8(&raw).map_err(|_| ApiError::internal())?;
        let recovery = recovery_digest(&v.code)
            .is_some_and(|code| auth.recovery.iter().any(|v| constant(v, &code)));
        if !recovery && verify_totp(secret, &v.code, now(), auth.mfa_last).is_none() {
            return Err(ApiError::new(400, "当前密码或验证码不正确"));
        }
        auth.mfa.clear();
        auth.recovery.clear();
        auth.mfa_last = 0;
    }
    auth.version = token();
    save_auth(&app, &mut i, auth)?;
    invalidate(&mut i);
    i.limits.sources.remove("mfa:account");
    app.persist_limits(&mut i)?;
    let x = i.new_session(&x.id, true)?;
    let name = i.auth.username.clone();
    app.record(&mut i, &format!("mfa_{action}"), &name);
    let mut info = i.info(&x);
    info["recoveryCodes"] = json!(recovery);
    Ok(ApiReply::session(info, &x.id))
}

pub fn recovery_digest(code: &str) -> Option<String> {
    use sha2::{Digest, Sha256};
    let code = code.replace('-', "").to_ascii_lowercase();
    if code.len() != 32 || !code.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(hex::encode(Sha256::digest(
        format!("yuji-recovery-v1:{code}").as_bytes(),
    )))
}
pub fn new_recovery(auth: &mut Auth) -> Vec<String> {
    let codes: Vec<String> = (0..10)
        .map(|_| {
            let raw = token()[..32].to_string();
            raw.as_bytes()
                .chunks(8)
                .map(|b| std::str::from_utf8(b).unwrap())
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect();
    auth.recovery = codes.iter().filter_map(|v| recovery_digest(v)).collect();
    codes
}
pub async fn elevate(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    let v: Mfa = decode(&body)?;
    let (hash, version) = {
        let mut i = app.lock();
        app.guard(&mut i, &c, true, true)?;
        reserve(&app, &mut i, "elevate:account", 5, false)?;
        (i.auth.hash.clone(), i.auth.version.clone())
    };
    let _slot = app
        .0
        .login_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::rate("请稍后重试", 2))?;
    if !password_matches(hash, v.password).await {
        return Err(ApiError::new(403, "密码或动态码不正确"));
    }
    let mut i = app.lock();
    app.guard(&mut i, &c, true, true)?;
    if i.auth.version != version {
        return Err(ApiError::new(409, "账户已变化，请重新登录"));
    }
    if !i.auth.mfa.is_empty() {
        let raw = app
            .unseal("admin:mfa", &i.auth.mfa)
            .map_err(|_| ApiError::internal())?;
        let secret = std::str::from_utf8(&raw).map_err(|_| ApiError::internal())?;
        let mut auth = i.auth.clone();
        if let Some(index) = recovery_digest(&v.code).and_then(|digest| {
            auth.recovery
                .iter()
                .position(|code| constant(code, &digest))
        }) {
            auth.recovery.remove(index);
            app.record(&mut i, "recovery_used", "敏感操作身份验证");
        } else {
            auth.mfa_last = verify_totp(secret, &v.code, now(), auth.mfa_last)
                .ok_or_else(|| ApiError::new(403, "密码或动态码不正确"))?;
        }
        save_auth(&app, &mut i, auth)?;
    }
    if let Some(s) = i.sessions.get_mut(&c.sid) {
        s.elevated = now();
    }
    i.limits.sources.remove("elevate:account");
    app.persist_limits(&mut i)?;
    app.record(&mut i, "reauthenticated", &c.ip);
    Ok(ApiReply::ok(json!({"ok":true,"expires":now()+300})))
}
pub fn require_elevated(i: &Inner, c: &Context) -> ApiResult<()> {
    if i.sessions
        .get(&c.sid)
        .is_some_and(|s| s.auth && now() - s.elevated < 300)
    {
        Ok(())
    } else {
        Err(ApiError::new(
            428,
            "请先验证当前密码与动态码，有效期为 5 分钟",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AccountFixture {
        app: App,
        session: Session,
        password: String,
    }
    impl AccountFixture {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("probe-username-{}", token()));
            std::fs::create_dir(&dir).unwrap();
            let password = token();
            atomic_json(
                &dir.join("auth.json"),
                &Auth {
                    username: "admin".into(),
                    hash: bcrypt::hash(&password, 4).unwrap(),
                    version: token(),
                    ..Default::default()
                },
            )
            .unwrap();
            atomic_json(
                &dir.join("nodes.json"),
                &crate::model::Data {
                    schema: 2,
                    ..Default::default()
                },
            )
            .unwrap();
            let app = App::new(dir, "https://account.test".into(), false).unwrap();
            let session = app.lock().new_session("", true).unwrap();
            Self {
                app,
                session,
                password,
            }
        }
        fn context(&self) -> Context {
            let mut h = axum::http::HeaderMap::new();
            h.insert(
                "Cookie",
                format!("{COOKIE}={}", self.session.id).parse().unwrap(),
            );
            h.insert("Origin", "https://account.test".parse().unwrap());
            h.insert("X-CSRF-Token", self.session.csrf.parse().unwrap());
            Context::new(
                "POST",
                "/api/admin/username",
                h,
                "127.0.0.1:9999".parse().unwrap(),
                &self.app.0.origin,
            )
        }
        async fn rename(&self, name: &str, password: &str) -> ApiResult<ApiReply> {
            change_username(
                self.app.clone(),
                self.context(),
                serde_json::to_vec(&json!({"username":name,"current":password})).unwrap(),
            )
            .await
        }
    }
    impl Drop for AccountFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.app.0.dir);
        }
    }
    #[tokio::test]
    async fn username_change_preserves_password_mfa_and_recovery_but_rotates_sessions() {
        let f = AccountFixture::new();
        let other = f.app.lock().new_session("", true).unwrap();
        let sealed = f
            .app
            .seal("admin:mfa", b"fixture authenticator secret")
            .unwrap();
        let recovery = token();
        let (hash, version) = {
            let mut i = f.app.lock();
            i.auth.mfa = sealed.clone();
            i.auth.mfa_last = 42;
            i.auth.recovery = vec![recovery.clone()];
            (i.auth.hash.clone(), i.auth.version.clone())
        };
        let reply = f.rename("operator.name-2", &f.password).await.unwrap();
        assert_eq!(reply.value["username"], "operator.name-2");
        assert_eq!(reply.value["authenticated"], true);
        assert_ne!(reply.value["csrf"].as_str().unwrap(), f.session.csrf);
        let stored: Auth = read_json(&f.app.0.dir.join("auth.json")).unwrap();
        assert_eq!(stored.username, "operator.name-2");
        assert_eq!(stored.hash, hash);
        assert_eq!(stored.mfa, sealed);
        assert_eq!(stored.mfa_last, 42);
        assert_eq!(stored.recovery, vec![recovery]);
        assert_ne!(stored.version, version);
        let mut i = f.app.lock();
        assert!(i.session(&f.session.id).is_none());
        assert!(i.session(&other.id).is_none());
        assert_eq!(i.sessions.len(), 1);
        assert_eq!(i.audit.last().unwrap().action, "username_changed");
    }
    #[tokio::test]
    async fn username_change_rejects_wrong_password_without_changing_account() {
        let f = AccountFixture::new();
        let version = f.app.lock().auth.version.clone();
        assert_eq!(
            f.rename("operator", "wrong password")
                .await
                .err()
                .unwrap()
                .status,
            400
        );
        let mut i = f.app.lock();
        assert_eq!(i.auth.username, "admin");
        assert_eq!(i.auth.version, version);
        assert!(i.session(&f.session.id).is_some());
    }
    #[tokio::test]
    async fn username_change_requires_admin_and_csrf() {
        let f = AccountFixture::new();
        let mut c = f.context();
        c.headers.remove("X-CSRF-Token");
        let raw = serde_json::to_vec(&json!({"username":"operator","current":f.password})).unwrap();
        assert_eq!(
            change_username(f.app.clone(), c, raw.clone())
                .await
                .err()
                .unwrap()
                .status,
            403
        );
        f.app.lock().sessions.get_mut(&f.session.id).unwrap().auth = false;
        assert_eq!(
            change_username(f.app.clone(), f.context(), raw)
                .await
                .err()
                .unwrap()
                .status,
            401
        );
        assert_eq!(f.app.lock().auth.username, "admin");
    }
    #[tokio::test]
    async fn username_change_rejects_invalid_or_unchanged_names() {
        let f = AccountFixture::new();
        for name in [
            "",
            "1admin",
            "a b",
            "root\nname",
            "管理员",
            "admin",
            "../other",
            "abcdefghijklmnopqrstuvwxyz0123456789",
        ] {
            assert_eq!(
                f.rename(name, &f.password).await.err().unwrap().status,
                400,
                "{name}"
            );
        }
        assert_eq!(f.app.lock().auth.username, "admin");
    }
    #[tokio::test]
    async fn username_change_shares_password_guessing_limit() {
        let f = AccountFixture::new();
        for _ in 0..5 {
            assert_eq!(
                f.rename("operator", "wrong").await.err().unwrap().status,
                400
            );
        }
        assert_eq!(
            f.rename("operator", &f.password)
                .await
                .err()
                .unwrap()
                .status,
            429
        );
        assert_eq!(f.app.lock().limits.sources["password:account"].count, 5);
    }
    #[test]
    fn username_payload_rejects_unrelated_security_fields() {
        assert!(
            decode::<UsernameInput>(br#"{"username":"operator","current":"test","mfa":""}"#)
                .is_err()
        );
    }
    #[test]
    fn rfc_totp_and_replay() {
        let secret = BASE32_NOPAD.encode(b"12345678901234567890");
        assert_eq!(totp(&secret, 1).as_deref(), Some("287082"));
        assert_eq!(verify_totp(&secret, "287082", 59, 0), Some(1));
        assert_eq!(verify_totp(&secret, "287082", 59, 1), None);
        assert_eq!(verify_totp(&secret, "28a082", 59, 0), None);
    }
    #[test]
    fn strict_login_payload() {
        assert!(decode::<Login>(br#"{"username":"admin","extra":true}"#).is_err());
        assert!(decode::<Login>(br#"{} {}"#).is_err());
    }
}
