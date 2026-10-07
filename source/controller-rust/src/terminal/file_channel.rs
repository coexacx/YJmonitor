//! Browser file transport bound to an already authorized, attached terminal.
use super::*;

struct Guard {
    terminal: TerminalGuard,
    lease: String,
}
impl Drop for Guard {
    fn drop(&mut self) {
        file_sessions::release_channel(&self.terminal.app, &self.lease, &self.terminal.id);
    }
}
fn authorize(
    app: &App,
    session: &Session,
    first: &Authorize,
) -> ApiResult<(
    crate::model::Node,
    Arc<crate::realtime::AgentLink>,
    crate::model::NodeSecret,
    CancellationToken,
    String,
    OwnedSemaphorePermit,
)> {
    let mut i = app.lock();
    let ticket = i
        .tickets
        .remove(&first.ticket)
        .filter(|t| t.session_id == session.id && t.expires >= now())
        .ok_or_else(|| ApiError::new(401, "文件通道授权已过期"))?;
    let node = i
        .data
        .nodes
        .iter()
        .find(|n| n.public.id == ticket.node_id && !n.removing)
        .cloned()
        .ok_or_else(|| ApiError::new(409, "节点当前不可用"))?;
    let link = i
        .agents
        .get(&ticket.node_id)
        .cloned()
        .ok_or_else(|| ApiError::new(409, "节点当前未连接"))?;
    if !i.terminal_valid(&session.id, &session.version, &ticket.node_id, &link) {
        return Err(ApiError::new(401, "文件通道授权已过期"));
    }
    let secret = i
        .data
        .secrets
        .get(&ticket.node_id)
        .cloned()
        .ok_or_else(|| ApiError::new(409, "节点认证配置不可用"))?;
    let slot = app
        .0
        .terminal_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::new(429, "连接数量已满"))?;
    let stop = app.0.stop.child_token();
    let id = token();
    file_sessions::attach_channel(
        &mut i,
        session,
        &node.public.id,
        &first.transfer_session,
        &id,
        stop.clone(),
    )?;
    i.terminals
        .entry(session.id.clone())
        .or_default()
        .insert(id.clone(), stop.clone());
    Ok((node, link, secret, stop, id, slot))
}
pub(super) async fn run(
    app: App,
    session: Session,
    pending: OwnedSemaphorePermit,
    mut ws: WebSocket,
    first: Authorize,
) {
    let (node, link, secret, stop, id, _slot) = match authorize(&app, &session, &first) {
        Ok(value) => value,
        Err(e) => {
            let _ = timeout(Duration::from_secs(2), ws.send(WS::Text(
                json!({"type":"error","message":e.message,"status":e.status,"retryable":e.status==409}).to_string().into()
            ))).await;
            return;
        }
    };
    drop(pending);
    let reason: EndReason = Arc::new(Mutex::new(None));
    let _guard = Guard {
        terminal: TerminalGuard {
            app: app.clone(),
            sid: session.id.clone(),
            id: id.clone(),
            name: format!("{} · 文件通道", node.public.name),
            file_sessions: String::new(),
            shell_session: String::new(),
            stop: stop.clone(),
            started: Instant::now(),
            reason: reason.clone(),
        },
        lease: first.transfer_session.clone(),
    };
    let check: files::Authorize = {
        let app = app.clone();
        let session = session.clone();
        let link = link.clone();
        let node = node.public.id.clone();
        let lease = first.transfer_session.clone();
        let id = id.clone();
        Arc::new(move || {
            let i = app.lock();
            i.terminal_valid(&session.id, &session.version, &node, &link)
                && file_sessions::channel_valid(&i, &session, &node, &lease, &id)
        })
    };
    let (mut writer, mut reader) = ws.split();
    let (out, mut output) = mpsc::channel::<WS>(4);
    let (control, mut controls) = mpsc::channel::<WS>(4);
    let writer_stop = stop.clone();
    let mut writer_task = tokio::spawn(async move {
        loop {
            let message = tokio::select! {
                biased;
                _=writer_stop.cancelled()=>break,
                message=async{tokio::select!{biased; Some(m)=controls.recv()=>Some(m),Some(m)=output.recv()=>Some(m),else=>None}}=>message,
            };
            let Some(message) = message else { break };
            let result = tokio::select! {
                _=writer_stop.cancelled()=>break,
                result=timeout(TERMINAL_IO_TIMEOUT,writer.send(message))=>result,
            };
            if !matches!(result, Ok(Ok(()))) {
                break;
            }
        }
        writer_stop.cancel();
        let _ = timeout(Duration::from_secs(1), writer.send(WS::Close(None))).await;
    });
    let (browse, browse_rx) = mpsc::channel::<files::Request>(2);
    let (transfer, transfer_rx) = mpsc::channel::<files::Request>(2);
    let context = FileConnection {
        app: app.clone(),
        node: node.clone(),
        secret,
        link,
        session: first.transfer_session.clone(),
        check: check.clone(),
        out: out.clone(),
        stop: stop.clone(),
    };
    let mut workers = vec![
        tokio::spawn(file_worker(context.clone(), browse_rx)),
        tokio::spawn(file_worker(context, transfer_rx)),
    ];
    let _=out.try_send(WS::Text(json!({"type":"ready","files":true,"terminal":false,"fileChannel":true,"transferSession":first.transfer_session}).to_string().into()));
    let mut heartbeat = Heartbeat::new(Instant::now());
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut window = Instant::now();
    let mut minute = window;
    let (mut frames, mut bytes, mut operations, mut control_frames) =
        (0usize, 0usize, 0usize, 0usize);
    loop {
        let next = tokio::select! {
            biased;
            _=stop.cancelled()=>break,
            _=tick.tick()=>{
                if !(check)(){ended(&reason,"authorization_lost");break}
                let at=Instant::now();
                if heartbeat.expired(at){ended(&reason,"heartbeat_timeout");break}
                if let Some(value)=heartbeat.ping(at){
                    match control.try_send(WS::Ping(value.into())){
                        Ok(())|Err(mpsc::error::TrySendError::Full(_))=>{},
                        Err(mpsc::error::TrySendError::Closed(_))=>break,
                    }
                }
                continue;
            },
            message=reader.next()=>message,
        };
        let Some(Ok(message)) = next else {
            ended(&reason, "browser_connection_lost");
            break;
        };
        if !(check)() {
            ended(&reason, "authorization_lost");
            break;
        }
        let at = Instant::now();
        if at.duration_since(window) >= Duration::from_secs(1) {
            window = at;
            frames = 0;
            bytes = 0;
            control_frames = 0;
        }
        if at.duration_since(minute) >= Duration::from_secs(60) {
            minute = at;
            operations = 0;
        }
        match message {
            WS::Pong(value) => {
                control_frames += 1;
                if control_frames > 16 {
                    break;
                }
                if heartbeat.acknowledge(&value, at) {
                    let mut i = app.lock();
                    if let Some(current) = i.sessions.get_mut(&session.id)
                        && current.auth
                        && current.version == session.version
                        && now() - current.created < 28800
                    {
                        current.seen = now();
                    }
                }
            }
            WS::Ping(value) => {
                control_frames += 1;
                if control_frames > 16 || control.try_send(WS::Pong(value)).is_err() {
                    break;
                }
            }
            WS::Text(raw) => {
                frames += 1;
                bytes = bytes.saturating_add(raw.len());
                if frames > 300 || bytes > files::MAX_FRAME {
                    break;
                }
                let Some(request) = files::parse(raw.as_bytes()) else {
                    break;
                };
                if files::is_inspection(&request) {
                    break;
                }
                if !files::is_chunk(&request) {
                    operations += 1
                }
                let validation = if operations > 90 {
                    Err(files::Problem {
                        code: "rate",
                        message: "文件操作过于频繁，请稍后重试".into(),
                    })
                } else {
                    files::validate(&request)
                };
                if let Err(error) = validation {
                    if out.try_send(files::response(&request, Err(error))).is_err() {
                        break;
                    }
                    continue;
                }
                let destination = if files::is_transfer(&request) {
                    &transfer
                } else {
                    &browse
                };
                if let Err(error) = destination.try_send(request) {
                    let request = error.into_inner();
                    if out
                        .try_send(files::response(
                            &request,
                            Err(files::Problem {
                                code: "busy",
                                message: "已有文件操作正在进行，请稍后重试".into(),
                            }),
                        ))
                        .is_err()
                    {
                        break;
                    }
                }
            }
            WS::Close(_) => {
                ended(&reason, "browser_closed");
                break;
            }
            WS::Binary(_) => {
                ended(&reason, "invalid_message");
                break;
            }
        }
    }
    stop.cancel();
    for worker in &mut workers {
        if timeout(Duration::from_secs(5), &mut *worker).await.is_err() {
            worker.abort();
            let _ = worker.await;
        }
    }
    if timeout(Duration::from_secs(2), &mut writer_task)
        .await
        .is_err()
    {
        writer_task.abort();
        let _ = writer_task.await;
    }
}
