//! Group notifications use the shared encrypted bot credential, with independently
//! persisted recipient, status and retry events. Telegram URLs are never logged.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Input {
    enabled: bool,
    chat_id: String,
}

async fn call(app: &App, secret: &str, method: &str, body: Value) -> ApiResult<Value> {
    let mut response = app
        .0
        .http
        .post(format!("https://api.telegram.org/bot{secret}/{method}"))
        .timeout(Duration::from_secs(5))
        .json(&body)
        .send()
        .await
        .map_err(|_| ApiError::new(502, "Telegram 连接失败或超时"))?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ApiError::new(502, "Telegram 响应中断"))?
    {
        if bytes.len() + chunk.len() > 65536 {
            return Err(ApiError::new(502, "Telegram 响应过大"));
        }
        bytes.extend(chunk);
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| ApiError::new(502, "Telegram 响应异常"))?;
    if !status.is_success() || value["ok"] != true {
        return Err(ApiError::new(
            if status.as_u16() == 429 || status.is_server_error() {
                502
            } else {
                400
            },
            "无法验证群组，请核对 Bot Token、群组 ID，以及机器人的管理员权限",
        ));
    }
    Ok(value["result"].clone())
}
fn identity(me: &Value) -> ApiResult<i64> {
    me["id"]
        .as_i64()
        .filter(|id| *id > 0 && me["is_bot"] == true)
        .ok_or_else(|| ApiError::new(400, "Bot 身份验证失败"))
}
fn group_details(chat: &Value, member: &Value, id: i64, chat_id: &str) -> ApiResult<String> {
    if chat["id"].as_i64().map(|v| v.to_string()).as_deref() != Some(chat_id)
        || !matches!(chat["type"].as_str(), Some("group" | "supergroup"))
        || member["user"]["id"].as_i64() != Some(id)
        || member["user"]["is_bot"] != true
        || !matches!(member["status"].as_str(), Some("administrator" | "creator"))
    {
        return Err(ApiError::new(400, "机器人必须已加入该群组并拥有管理员权限"));
    }
    Ok(chat["title"]
        .as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(128)
        .collect())
}
pub(super) async fn verify(app: &App, secret: &str, chat_id: &str) -> ApiResult<String> {
    if !valid_token(secret) || !valid_chat(chat_id) || !chat_id.starts_with('-') {
        return Err(ApiError::new(400, "请保存有效的 Bot Token 和负数群组 ID"));
    }
    let id = identity(&call(app, secret, "getMe", json!({})).await?)?;
    let chat = call(app, secret, "getChat", json!({"chat_id":chat_id})).await?;
    let member = call(
        app,
        secret,
        "getChatMember",
        json!({"chat_id":chat_id,"user_id":id}),
    )
    .await?;
    group_details(&chat, &member, id, chat_id)
}
pub async fn api(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    if c.path == "/api/admin/telegram/group/test" && c.method == "POST" {
        let _: Empty = decode(&body)?;
        let mut i = app.lock();
        app.guard(&mut i, &c, true, true)?;
        if !i.telegram.group.enabled {
            return Err(ApiError::new(400, "请先验证并启用群组通知"));
        }
        if now() - i.telegram.group.last_test_at < 60 {
            return Err(ApiError::rate("每 60 秒可发送一次群组测试通知", 60));
        }
        if i.telegram.queue.len() >= LIMIT {
            return Err(ApiError::rate("通知队列已满", 60));
        }
        let mut next = i.telegram.clone();
        let id = token()[..24].to_string();
        next.queue.push(TelegramEvent {
            id: id.clone(),
            group: true,
            kind: "test".into(),
            site: i.data.site.name.clone(),
            at: now(),
            ..Default::default()
        });
        next.group.test = TelegramTestResult {
            id: id.clone(),
            status: "pending".into(),
            error: String::new(),
        };
        next.group.last_test_at = now();
        if !save(&app, &mut i, next) {
            return Err(ApiError::internal());
        }
        app.record(&mut i, "telegram_test", "群组测试通知");
        return Ok(ApiReply::accepted(json!({"id":id})));
    }
    if c.path != "/api/admin/telegram/group" || c.method != "PUT" {
        return Err(ApiError::new(405, "不支持的请求方式"));
    }
    let mut input: Input = decode(&body)?;
    input.chat_id = input.chat_id.trim().into();
    if !input.chat_id.is_empty() && (!valid_chat(&input.chat_id) || !input.chat_id.starts_with('-'))
    {
        return Err(ApiError::new(400, "群组 ID 必须为有效负数"));
    }
    let (encrypted, revision, session) = {
        let mut i = app.lock();
        let session = app.guard(&mut i, &c, true, true)?;
        i.request(&format!("telegram-group:{}", session.handle), 4)?;
        (
            i.telegram.config.token.clone(),
            i.telegram_revision,
            session,
        )
    };
    let title = if input.enabled {
        let raw = app
            .unseal(LABEL, &encrypted)
            .map_err(|_| ApiError::new(400, "请先保存 Bot Token"))?;
        let secret =
            std::str::from_utf8(&raw).map_err(|_| ApiError::new(400, "Bot Token 不可用"))?;
        verify(&app, secret, &input.chat_id).await?
    } else {
        String::new()
    };
    let mut i = app.lock();
    let current = app.guard(&mut i, &c, true, true)?;
    if current.id != session.id
        || current.version != session.version
        || i.telegram_revision != revision
    {
        return Err(ApiError::new(409, "设置已发生变化，请刷新后重试"));
    }
    let mut next = i.telegram.clone();
    next.group = TelegramGroup {
        enabled: input.enabled,
        chat_id: input.chat_id,
        title,
        verified_at: if input.enabled { now() } else { 0 },
        last_test_at: next.group.last_test_at,
        ..Default::default()
    };
    next.queue.retain(|e| !e.group);
    // Establish a baseline; enabling notifications does not announce every node.
    if !next.config.enabled {
        next.nodes = i
            .data
            .nodes
            .iter()
            .filter(|n| !n.demo && !n.removing)
            .map(|n| {
                (
                    n.public.id.clone(),
                    TelegramNodeState {
                        online: n.public.online,
                        was_offline: !n.public.online,
                        offline_since: 0,
                    },
                )
            })
            .collect();
    }
    if !save(&app, &mut i, next) {
        return Err(ApiError::internal());
    }
    if let Some((_, cancel)) = &i.delivery {
        cancel.cancel();
    }
    i.telegram_revision += 1;
    app.record(&mut i, "telegram_updated", "群组通知设置");
    Ok(ApiReply::ok(view(&i)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requires_bot_admin_in_exact_group() {
        let chat = json!({"id":-100123,"type":"supergroup","title":"Group"});
        let member = json!({"status":"administrator","user":{"id":123456,"is_bot":true}});
        assert_eq!(
            group_details(&chat, &member, 123456, "-100123").unwrap(),
            "Group"
        );
        for status in ["member", "restricted", "left", "kicked"] {
            let mut m = member.clone();
            m["status"] = json!(status);
            assert!(group_details(&chat, &m, 123456, "-100123").is_err());
        }
        for kind in ["private", "channel"] {
            let mut c = chat.clone();
            c["type"] = json!(kind);
            assert!(group_details(&c, &member, 123456, "-100123").is_err());
        }
        assert!(group_details(&chat, &member, 999, "-100123").is_err());
        assert!(group_details(&chat, &member, 123456, "-100999").is_err());
        assert!(identity(&json!({"id":123456,"is_bot":false})).is_err());
    }
    #[test]
    fn retry_order_is_independent_by_destination() {
        let queue = vec![
            TelegramEvent {
                group: false,
                next_attempt: 100,
                ..Default::default()
            },
            TelegramEvent {
                group: false,
                next_attempt: 0,
                ..Default::default()
            },
            TelegramEvent {
                group: true,
                next_attempt: 0,
                ..Default::default()
            },
        ];
        assert_eq!(due_index(&queue, 50), Some(2));
        assert_eq!(due_index(&queue, 100), Some(0));
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    #[tokio::test]
    async fn group_receives_status_only_and_preserves_personal_settings() {
        let dir = std::env::temp_dir().join(format!("probe-telegram-group-{}", token()));
        std::fs::create_dir(&dir).unwrap();
        atomic_json(&dir.join("auth.json"), &Auth::default()).unwrap();
        atomic_json(
            &dir.join("nodes.json"),
            &crate::model::Data {
                schema: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let app = App::new(dir.clone(), "https://example.invalid".into(), true).unwrap();
        {
            let mut i = app.lock();
            i.telegram_ready = 0;
            i.telegram.group.enabled = true;
            i.telegram.config.enabled = false;
            i.telegram.config.login = true;
            i.telegram.config.renewal = true;
            i.data.nodes.push(Node {
                public: PublicNode {
                    id: "one".into(),
                    name: "<node>".into(),
                    online: false,
                    ..Default::default()
                },
                last_seen: now() - 60,
                notify_renewal: true,
                expires_at: (chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339(),
                renewal_version: token(),
                ..Default::default()
            });
            i.telegram.nodes.insert(
                "one".into(),
                TelegramNodeState {
                    online: true,
                    offline_since: now() - 21,
                    ..Default::default()
                },
            );
            tick(&app, &mut i);
            assert_eq!(i.telegram.queue.len(), 1);
            assert!(i.telegram.queue[0].group);
            assert_eq!(i.telegram.queue[0].kind, "offline");
            i.data.nodes[0].public.online = true;
            i.data.nodes[0].last_seen = now();
            tick(&app, &mut i);
            assert_eq!(i.telegram.queue.len(), 2);
            assert!(i.telegram.queue[1].recovery);
            assert!(text(&i.telegram.queue[1]).contains("服务器恢复"));
            queue_notice(&app, &mut i, "private login source", "security");
            assert_eq!(i.telegram.queue.len(), 2);
            for kind in ["security", "renewal", "resource"] {
                let e = TelegramEvent {
                    group: true,
                    kind: kind.into(),
                    ..Default::default()
                };
                assert!(!event_current(&i, &e));
            }
            i.telegram.group.enabled = false;
            invalidate(&app, &mut i);
            assert!(i.telegram.queue.is_empty());
            assert!(i.telegram.config.login && i.telegram.config.renewal);
            let legacy: TelegramState =
                serde_json::from_value(json!({"config":{"enabled":true,"chatId":"12345"}}))
                    .unwrap();
            assert!(!legacy.group.enabled);
            assert_eq!(legacy.config.chat_id, "12345");
        }
        drop(app);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
