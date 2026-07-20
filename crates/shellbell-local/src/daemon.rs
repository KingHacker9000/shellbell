use crate::{
    ActivityEngine, EffectiveConfig, LocalPaths, Mode,
    config::load_credentials,
    ipc::{
        CLI_TIMEOUT, DaemonStatus, IPC_VERSION, IpcCommand, IpcRequest, IpcResponse, IpcResult,
        MAX_IPC_MESSAGE, SessionStatus, decode_request, encode_line,
    },
    store::{QueuedRing, Store},
};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use nix::{
    errno::Errno,
    sys::signal::kill,
    unistd::{Pid, Uid},
};
use rand::Rng;
use reqwest::{Client, StatusCode};
use shellbell_protocol::{RingAcceptedResponse, RingRequest};
use std::{
    fs,
    future::Future,
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{Mutex, broadcast},
    time::{interval, timeout},
};
use url::Url;
use uuid::Uuid;

const SESSION_STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone)]
struct DaemonContext {
    engine: Arc<Mutex<ActivityEngine>>,
    store: Store,
    config: EffectiveConfig,
    paths: LocalPaths,
    started: Instant,
    client: Client,
    shutdown: broadcast::Sender<()>,
}

impl DaemonContext {
    fn mono_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

pub async fn run_daemon(paths: LocalPaths) -> Result<()> {
    run_daemon_until(paths, shutdown_signal()).await
}

pub async fn run_daemon_until<F>(paths: LocalPaths, shutdown: F) -> Result<()>
where
    F: Future<Output = ()>,
{
    paths.ensure_private_dirs()?;
    let config = EffectiveConfig::load(&paths.config_file())?;
    let store = Store::open(&paths.database_file()).await?;
    let started = Instant::now();
    let now_wall = wall_ms();
    let mut sessions = Vec::new();
    for mut session in store.load_sessions().await? {
        if pid_is_alive(session.shell_pid)
            && now_wall.saturating_sub(session.last_seen_wall_ms)
                <= i64::try_from(SESSION_STALE_AFTER.as_millis()).unwrap_or(i64::MAX)
        {
            session.recover_live_timing(0, now_wall);
            sessions.push(session);
        }
    }
    let engine = ActivityEngine::from_sessions(sessions);
    store.save_sessions(engine.sessions()).await?;

    prepare_socket(&paths.socket_file()).await?;
    let listener = UnixListener::bind(paths.socket_file())
        .with_context(|| format!("could not bind {}", paths.socket_file().display()))?;
    fs::set_permissions(paths.socket_file(), fs::Permissions::from_mode(0o600))?;

    let (shutdown_sender, mut daemon_shutdown) = broadcast::channel(1);
    let context = DaemonContext {
        engine: Arc::new(Mutex::new(engine)),
        store,
        config,
        paths: paths.clone(),
        started,
        client: Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("shellbell-daemon/", env!("CARGO_PKG_VERSION")))
            .build()?,
        shutdown: shutdown_sender,
    };
    let mut timer_tick = interval(Duration::from_millis(100));
    let mut delivery_tick = interval(Duration::from_secs(1));
    let mut cleanup_tick = interval(Duration::from_secs(60));
    tokio::pin!(shutdown);

    let result = loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        let context = context.clone();
                        tokio::spawn(async move {
                            let _ = serve_connection(stream, context).await;
                        });
                    }
                    Err(error) => break Err(error.into()),
                }
            }
            _ = timer_tick.tick() => {
                if let Err(error) = process_timers(&context).await {
                    break Err(error);
                }
            }
            _ = delivery_tick.tick() => {
                if let Err(error) = deliver_one(&context).await {
                    // A local store failure is fatal; network failures are persisted by deliver_one.
                    break Err(error);
                }
            }
            _ = cleanup_tick.tick() => {
                if let Err(error) = cleanup_sessions(&context).await {
                    break Err(error);
                }
            }
            () = &mut shutdown => break Ok(()),
            _ = daemon_shutdown.recv() => break Ok(()),
        }
    };
    {
        let engine = context.engine.lock().await;
        context.store.save_sessions(engine.sessions()).await?;
    }
    let _ = fs::remove_file(paths.socket_file());
    result
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler can be installed");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

async fn prepare_socket(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let ping = IpcRequest::new(IpcCommand::DaemonPing);
    if crate::ipc::request(path, &ping, Duration::from_millis(150))
        .await
        .is_ok()
    {
        bail!("Shellbell daemon is already running");
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_socket() {
        bail!("refusing to replace non-socket path {}", path.display());
    }
    fs::remove_file(path)?;
    Ok(())
}

async fn serve_connection(mut stream: UnixStream, context: DaemonContext) -> Result<()> {
    let peer = stream
        .peer_cred()
        .context("could not inspect local IPC peer")?;
    if !peer_is_authorized(peer.uid()) {
        bail!("local IPC peer belongs to another user");
    }
    let mut bytes = Vec::new();
    let read = timeout(Duration::from_millis(250), async {
        let mut reader = BufReader::new(&mut stream).take((MAX_IPC_MESSAGE + 1) as u64);
        reader.read_until(b'\n', &mut bytes).await
    })
    .await
    .context("local IPC read timed out")??;
    if read == 0 {
        bail!("empty local IPC request");
    }
    let response = match decode_request(bytes.strip_suffix(b"\n").unwrap_or(&bytes)) {
        Ok(request) => {
            let request_id = request.request_id;
            match handle_request(request, &context).await {
                Ok(result) => IpcResponse::success(request_id, result),
                Err(error) => IpcResponse::failure(request_id, error.to_string()),
            }
        }
        Err(error) => IpcResponse::failure(Uuid::nil(), error.to_string()),
    };
    stream.write_all(&encode_line(&response)?).await?;
    stream.shutdown().await?;
    Ok(())
}

fn peer_is_authorized(uid: u32) -> bool {
    uid == Uid::effective().as_raw()
}

async fn handle_request(request: IpcRequest, context: &DaemonContext) -> Result<IpcResult> {
    if request.version != IPC_VERSION {
        bail!("local IPC version mismatch");
    }
    let now_mono = context.mono_ms();
    let now_wall = wall_ms();
    let mut engine = context.engine.lock().await;
    let result = match request.command {
        IpcCommand::SessionOpen {
            session_id,
            shell_pid,
            shell_type,
            tty,
        } => {
            engine.session_open(
                session_id,
                shell_pid,
                shell_type,
                tty,
                duration_ms(context.config.minimum_active),
                duration_ms(context.config.idle_for),
                context.config.targets.clone(),
                now_wall,
            );
            IpcResult::Acknowledged
        }
        IpcCommand::CommandStart { session_id } => {
            require_session(engine.command_start(session_id, now_mono, now_wall))?;
            IpcResult::Acknowledged
        }
        IpcCommand::PromptReady { session_id } => {
            require_session(engine.prompt_ready(session_id, now_mono, now_wall))?;
            IpcResult::Acknowledged
        }
        IpcCommand::SessionClose { session_id } => {
            engine.session_close(session_id);
            IpcResult::Acknowledged
        }
        IpcCommand::ArmPersistent {
            session_id,
            minimum_active_ms,
            idle_for_ms,
            targets,
        } => {
            require_session(engine.arm_and_start(
                session_id,
                Mode::Persistent,
                minimum_active_ms,
                idle_for_ms,
                targets,
                now_mono,
                now_wall,
            ))?;
            IpcResult::Acknowledged
        }
        IpcCommand::ArmOnce {
            session_id,
            minimum_active_ms,
            idle_for_ms,
            targets,
        } => {
            require_session(engine.arm_and_start(
                session_id,
                Mode::Once,
                minimum_active_ms,
                idle_for_ms,
                targets,
                now_mono,
                now_wall,
            ))?;
            IpcResult::Acknowledged
        }
        IpcCommand::Disarm { session_id } => {
            require_session(engine.disarm(session_id, now_wall))?;
            IpcResult::Acknowledged
        }
        IpcCommand::SetLabel { session_id, label } => {
            require_session(engine.set_label(session_id, label, now_wall))?;
            IpcResult::Acknowledged
        }
        IpcCommand::Status { session_id } => {
            let session = engine
                .get(session_id)
                .context("shell session is not registered; start a new integrated shell")?;
            return Ok(IpcResult::Status {
                session: SessionStatus::from_session(session, now_mono),
            });
        }
        IpcCommand::ManualRing {
            session_id,
            message,
            targets,
        } => {
            let action = engine.manual_ring(session_id, message, targets, now_mono, now_wall);
            if let Some(action) = action {
                context
                    .store
                    .save_sessions_and_enqueue(
                        engine.sessions(),
                        std::slice::from_ref(&action),
                        now_wall,
                        context.config.queue_limit,
                        context.config.event_max_age,
                    )
                    .await?;
                let event_id = action.event_id;
                return Ok(IpcResult::RingQueued {
                    event_id: Some(event_id),
                });
            }
            IpcResult::RingQueued { event_id: None }
        }
        IpcCommand::DaemonPing => {
            let status = context
                .store
                .daemon_status(engine.sessions().count())
                .await?;
            return Ok(IpcResult::Pong { daemon: status });
        }
        IpcCommand::DaemonShutdown => {
            let _ = context.shutdown.send(());
            return Ok(IpcResult::Acknowledged);
        }
    };
    context.store.save_sessions(engine.sessions()).await?;
    Ok(result)
}

fn require_session(found: bool) -> Result<()> {
    if !found {
        bail!("shell session is not registered; start a new integrated shell");
    }
    Ok(())
}

async fn process_timers(context: &DaemonContext) -> Result<()> {
    let now_mono = context.mono_ms();
    let now_wall = wall_ms();
    let mut engine = context.engine.lock().await;
    let mut actions = engine.tick(now_mono, now_wall);
    for action in &mut actions {
        if action.automatic && action.message.as_deref() == Some("Shell is ready") {
            action.message = Some(context.config.automatic_message.clone());
        }
    }
    if !actions.is_empty() {
        context
            .store
            .save_sessions_and_enqueue(
                engine.sessions(),
                &actions,
                now_wall,
                context.config.queue_limit,
                context.config.event_max_age,
            )
            .await?;
    }
    Ok(())
}

async fn cleanup_sessions(context: &DaemonContext) -> Result<()> {
    let now = wall_ms();
    let stale_cutoff =
        now.saturating_sub(i64::try_from(SESSION_STALE_AFTER.as_millis()).unwrap_or(i64::MAX));
    let mut engine = context.engine.lock().await;
    let mut removed = engine.cleanup_stale(stale_cutoff);
    let dead: Vec<Uuid> = engine
        .sessions()
        .filter(|session| !pid_is_alive(session.shell_pid))
        .map(|session| session.id)
        .collect();
    for id in dead {
        if engine.session_close(id) {
            removed.push(id);
        }
    }
    if !removed.is_empty() {
        context.store.save_sessions(engine.sessions()).await?;
    }
    context
        .store
        .expire_old(now, context.config.event_max_age)
        .await?;
    Ok(())
}

async fn deliver_one(context: &DaemonContext) -> Result<()> {
    let now = wall_ms();
    let Some(event) = context
        .store
        .next_due(now, context.config.event_max_age)
        .await?
    else {
        return Ok(());
    };
    match submit_ring(context, &event).await {
        DeliveryResult::Accepted => context.store.mark_delivered(event.event_id).await?,
        DeliveryResult::AuthorizationFailure(diagnostic) => {
            context
                .store
                .mark_authorization_failure(event.event_id, &diagnostic)
                .await?;
        }
        DeliveryResult::PermanentFailure => {
            context.store.discard_permanent(event.event_id).await?;
        }
        DeliveryResult::TransientFailure(diagnostic) => {
            let delay = retry_delay(event.attempts);
            context
                .store
                .mark_retry(
                    event.event_id,
                    now.saturating_add(i64::try_from(delay.as_millis()).unwrap_or(i64::MAX)),
                    &diagnostic,
                )
                .await?;
        }
    }
    Ok(())
}

enum DeliveryResult {
    Accepted,
    AuthorizationFailure(String),
    PermanentFailure,
    TransientFailure(String),
}

async fn submit_ring(context: &DaemonContext, event: &QueuedRing) -> DeliveryResult {
    let credentials = match load_credentials(&context.paths.credentials_file()) {
        Ok(credentials) => credentials,
        Err(error) => return DeliveryResult::TransientFailure(error.to_string()),
    };
    let endpoint = match endpoint(&credentials.relay_url, "api/rings") {
        Ok(endpoint) => endpoint,
        Err(_error) => return DeliveryResult::PermanentFailure,
    };
    let response = match context
        .client
        .post(endpoint)
        .bearer_auth(&credentials.source_token)
        .json(&RingRequest {
            event_id: event.event_id,
            message: event.message.clone(),
            target_tags: event.targets.clone(),
        })
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => return DeliveryResult::TransientFailure(network_diagnostic(&error)),
    };
    let status = response.status();
    if status.is_success() {
        return match response.json::<RingAcceptedResponse>().await {
            Ok(accepted) if accepted.event_id == event.event_id => DeliveryResult::Accepted,
            _ => DeliveryResult::TransientFailure("relay returned an invalid response".into()),
        };
    }
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return DeliveryResult::AuthorizationFailure(format!("relay returned HTTP {status}"));
    }
    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        return DeliveryResult::TransientFailure(format!("relay returned HTTP {status}"));
    }
    DeliveryResult::PermanentFailure
}

fn network_diagnostic(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "relay request timed out".into()
    } else if error.is_connect() {
        "relay connection failed".into()
    } else {
        "relay request failed".into()
    }
}

fn endpoint(base: &Url, path: &str) -> Result<Url> {
    Ok(base.join(path)?)
}

fn retry_delay(attempts: u32) -> Duration {
    let exponent = attempts.min(8);
    let base_ms = 5_000_u64
        .saturating_mul(1_u64 << exponent)
        .min(15 * 60 * 1_000);
    let jitter = rand::rng().random_range(0..=base_ms / 4);
    Duration::from_millis(base_ms.saturating_add(jitter))
}

fn duration_ms(value: Duration) -> u64 {
    u64::try_from(value.as_millis()).unwrap_or(u64::MAX)
}

fn wall_ms() -> i64 {
    Utc::now().timestamp_millis()
}

pub fn pid_is_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    match kill(Pid::from_raw(pid), None) {
        Ok(()) | Err(Errno::EPERM) => true,
        Err(_) => false,
    }
}

pub async fn ping(paths: &LocalPaths) -> Result<DaemonStatus> {
    let response = crate::ipc::request(
        &paths.socket_file(),
        &IpcRequest::new(IpcCommand::DaemonPing),
        CLI_TIMEOUT,
    )
    .await?;
    match response.result {
        Some(IpcResult::Pong { daemon }) => Ok(daemon),
        _ => bail!("daemon returned an unexpected ping response"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ShellType, ipc};
    use tokio::sync::oneshot;

    fn test_paths(directory: &Path) -> LocalPaths {
        LocalPaths {
            config_dir: directory.join("config"),
            state_dir: directory.join("state"),
            runtime_dir: directory.join("run"),
        }
    }

    async fn wait_for_socket(path: &Path) {
        for _ in 0..100 {
            if path.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("daemon socket was not created");
    }

    #[tokio::test]
    async fn ipc_round_trip_status_and_multiple_clients() {
        let directory = tempfile::tempdir().unwrap();
        let paths = test_paths(directory.path());
        let (stop, stopped) = oneshot::channel();
        let task_paths = paths.clone();
        let daemon = tokio::spawn(async move {
            run_daemon_until(task_paths, async {
                let _ = stopped.await;
            })
            .await
        });
        wait_for_socket(&paths.socket_file()).await;
        let id = Uuid::new_v4();
        let opened = IpcRequest::new(IpcCommand::SessionOpen {
            session_id: id,
            shell_pid: std::process::id(),
            shell_type: ShellType::Bash,
            tty: None,
        });
        ipc::request(&paths.socket_file(), &opened, CLI_TIMEOUT)
            .await
            .unwrap();
        let requests: Vec<_> = (0..8)
            .map(|_| {
                let socket = paths.socket_file();
                async move {
                    ipc::request(
                        &socket,
                        &IpcRequest::new(IpcCommand::Status { session_id: id }),
                        CLI_TIMEOUT,
                    )
                    .await
                }
            })
            .collect();
        for request in requests {
            let response = request.await.unwrap();
            assert!(matches!(response.result, Some(IpcResult::Status { .. })));
        }
        let status = ping(&paths).await.unwrap();
        assert_eq!(status.active_sessions, 1);
        stop.send(()).unwrap();
        daemon.await.unwrap().unwrap();
        assert!(!paths.socket_file().exists());
    }

    #[tokio::test]
    async fn malformed_and_oversized_clients_are_bounded_and_daemon_survives() {
        let directory = tempfile::tempdir().unwrap();
        let paths = test_paths(directory.path());
        let (stop, stopped) = oneshot::channel();
        let task_paths = paths.clone();
        let daemon = tokio::spawn(async move {
            run_daemon_until(task_paths, async {
                let _ = stopped.await;
            })
            .await
        });
        wait_for_socket(&paths.socket_file()).await;
        for payload in [b"not-json\n".to_vec(), vec![b'x'; MAX_IPC_MESSAGE + 2]] {
            let mut stream = UnixStream::connect(paths.socket_file()).await.unwrap();
            stream.write_all(&payload).await.unwrap();
            let _ = stream.shutdown().await;
        }
        let _hanging = UnixStream::connect(paths.socket_file()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(ping(&paths).await.unwrap().active_sessions, 0);
        stop.send(()).unwrap();
        daemon.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn unavailable_daemon_fails_quickly_and_socket_permissions_are_private() {
        let directory = tempfile::tempdir().unwrap();
        let paths = test_paths(directory.path());
        let started = Instant::now();
        assert!(ping(&paths).await.is_err());
        assert!(started.elapsed() < CLI_TIMEOUT);

        let (stop, stopped) = oneshot::channel();
        let task_paths = paths.clone();
        let daemon = tokio::spawn(async move {
            run_daemon_until(task_paths, async {
                let _ = stopped.await;
            })
            .await
        });
        wait_for_socket(&paths.socket_file()).await;
        assert_eq!(
            fs::metadata(paths.socket_file())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(peer_is_authorized(Uid::effective().as_raw()));
        assert!(!peer_is_authorized(
            Uid::effective().as_raw().saturating_add(1)
        ));
        stop.send(()).unwrap();
        daemon.await.unwrap().unwrap();
    }

    #[test]
    fn retry_is_bounded_and_process_liveness_is_safe() {
        assert!(retry_delay(0) >= Duration::from_secs(5));
        assert!(retry_delay(100) <= Duration::from_secs(15 * 60 + 225));
        assert!(pid_is_alive(std::process::id()));
        assert!(!pid_is_alive(u32::MAX));
    }

    #[tokio::test]
    async fn once_manual_ring_is_associated_and_queued_without_duplicate() {
        let directory = tempfile::tempdir().unwrap();
        let paths = test_paths(directory.path());
        let (stop, stopped) = oneshot::channel();
        let task_paths = paths.clone();
        let daemon = tokio::spawn(async move {
            run_daemon_until(task_paths, async {
                let _ = stopped.await;
            })
            .await
        });
        wait_for_socket(&paths.socket_file()).await;
        let id = Uuid::new_v4();
        for command in [
            IpcCommand::SessionOpen {
                session_id: id,
                shell_pid: std::process::id(),
                shell_type: ShellType::Fish,
                tty: None,
            },
            IpcCommand::ArmOnce {
                session_id: id,
                minimum_active_ms: 100,
                idle_for_ms: 100,
                targets: vec!["phone".into()],
            },
            IpcCommand::CommandStart { session_id: id },
        ] {
            ipc::request(&paths.socket_file(), &IpcRequest::new(command), CLI_TIMEOUT)
                .await
                .unwrap();
        }
        let first = ipc::request(
            &paths.socket_file(),
            &IpcRequest::new(IpcCommand::ManualRing {
                session_id: Some(id),
                message: Some("ready".into()),
                targets: vec![],
            }),
            CLI_TIMEOUT,
        )
        .await
        .unwrap();
        assert!(matches!(
            first.result,
            Some(IpcResult::RingQueued { event_id: Some(_) })
        ));
        let response = ipc::request(
            &paths.socket_file(),
            &IpcRequest::new(IpcCommand::Status { session_id: id }),
            CLI_TIMEOUT,
        )
        .await
        .unwrap();
        let Some(IpcResult::Status { session }) = response.result else {
            panic!("expected status");
        };
        assert_eq!(session.mode, Mode::Off);
        assert_eq!(ping(&paths).await.unwrap().pending_events, 1);
        stop.send(()).unwrap();
        daemon.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn daemon_restart_recovers_settling_and_queues_once() {
        let directory = tempfile::tempdir().unwrap();
        let paths = test_paths(directory.path());
        fs::create_dir_all(&paths.config_dir).unwrap();
        fs::write(
            paths.config_file(),
            "[activity]\nminimum_active = \"100ms\"\nidle_for = \"400ms\"\n",
        )
        .unwrap();
        let id = Uuid::new_v4();
        let (stop_first, stopped_first) = oneshot::channel();
        let first_paths = paths.clone();
        let first = tokio::spawn(async move {
            run_daemon_until(first_paths, async {
                let _ = stopped_first.await;
            })
            .await
        });
        wait_for_socket(&paths.socket_file()).await;
        for command in [
            IpcCommand::SessionOpen {
                session_id: id,
                shell_pid: std::process::id(),
                shell_type: ShellType::Bash,
                tty: None,
            },
            IpcCommand::ArmPersistent {
                session_id: id,
                minimum_active_ms: 100,
                idle_for_ms: 400,
                targets: vec!["phone".into()],
            },
            IpcCommand::CommandStart { session_id: id },
        ] {
            ipc::request(&paths.socket_file(), &IpcRequest::new(command), CLI_TIMEOUT)
                .await
                .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
        ipc::request(
            &paths.socket_file(),
            &IpcRequest::new(IpcCommand::PromptReady { session_id: id }),
            CLI_TIMEOUT,
        )
        .await
        .unwrap();
        stop_first.send(()).unwrap();
        first.await.unwrap().unwrap();

        let (stop_second, stopped_second) = oneshot::channel();
        let second_paths = paths.clone();
        let second = tokio::spawn(async move {
            run_daemon_until(second_paths, async {
                let _ = stopped_second.await;
            })
            .await
        });
        wait_for_socket(&paths.socket_file()).await;
        tokio::time::sleep(Duration::from_millis(450)).await;
        assert_eq!(ping(&paths).await.unwrap().pending_events, 1);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(ping(&paths).await.unwrap().pending_events, 1);
        stop_second.send(()).unwrap();
        second.await.unwrap().unwrap();
    }
}
