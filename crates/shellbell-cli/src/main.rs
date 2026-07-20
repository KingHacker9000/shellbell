use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use shellbell_local::{
    EffectiveConfig, LocalPaths, ShellType, StoredCredentials,
    config::{load_credentials, parse_session_duration, save_credentials},
    daemon,
    install::{self, InstallOptions, STARTUP_MARKER},
    ipc::{self, CLI_TIMEOUT, HOOK_TIMEOUT, IpcCommand, IpcRequest, IpcResult},
};
use shellbell_protocol::{
    HealthResponse, PairingCreateRequest, PairingCreateResponse, PairingPollResponse, PairingState,
    SourceView,
};
use std::{
    env, fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    time::Duration,
};
use tokio::time::{Instant, sleep};
use url::Url;
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "shellbell",
    version,
    about = "Private terminal attention notifier"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pair this source machine with a Shellbell relay.
    Pair { relay_url: Url },
    /// Install managed Bash, Zsh, and/or Fish integration.
    Install(InstallCommand),
    /// Remove only Shellbell-managed shell and service integration.
    Uninstall(UninstallCommand),
    /// Persistently arm this interactive shell session.
    On(ArmCommand),
    /// Arm this interactive shell session for one qualifying ring.
    Once(ArmCommand),
    /// Disarm this interactive shell session.
    Off,
    /// Show activity state for this interactive shell session.
    Status,
    /// Set a session-only label; omit the label to clear it.
    Name { label: Option<String> },
    /// Send a manual notification without inspecting command content.
    Ring {
        message: Option<String>,
        #[arg(long, value_enum, default_value = "all")]
        to: Target,
    },
    /// Print the resolved safe, effective local configuration.
    Config,
    /// Check relay, daemon, service, and shell integration health.
    Doctor,
    #[command(name = "__daemon", hide = true)]
    Daemon,
    #[command(name = "__hook", hide = true)]
    Hook(HookCommand),
    #[command(name = "__session-id", hide = true)]
    SessionId,
    #[command(name = "__shutdown", hide = true)]
    Shutdown,
}

#[derive(Args)]
struct InstallCommand {
    /// Install one selected shell; may be repeated.
    #[arg(long = "shell", value_enum)]
    shells: Vec<ShellChoice>,
    /// Install all three supported shell integrations.
    #[arg(long, conflicts_with = "shells")]
    all_shells: bool,
    /// Show changes without writing files or managing the service.
    #[arg(long)]
    dry_run: bool,
    /// Alternate absolute home used for safe inspection and tests.
    #[arg(long)]
    home: Option<PathBuf>,
}

#[derive(Args)]
struct UninstallCommand {
    /// Show changes without writing files or managing the service.
    #[arg(long)]
    dry_run: bool,
    /// Alternate absolute home used for safe inspection and tests.
    #[arg(long)]
    home: Option<PathBuf>,
    /// Also remove local state, settings, and paired source credentials.
    #[arg(long)]
    purge: bool,
}

#[derive(Args)]
struct ArmCommand {
    /// Foreground activity required for the burst to qualify (for example 30s or 5m).
    #[arg(long = "after")]
    minimum_active: Option<String>,
    /// Prompt-ready idle window before ringing (for example 10s or 1m).
    #[arg(long = "idle")]
    idle_for: Option<String>,
    /// Override receiver targets for this session without changing global config.
    #[arg(long, value_enum)]
    to: Option<Target>,
}

#[derive(Args)]
struct HookCommand {
    #[arg(value_enum)]
    event: HookEvent,
    #[arg(long, value_enum)]
    shell: Option<ShellChoice>,
    #[arg(long)]
    pid: Option<u32>,
    #[arg(long)]
    tty: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Target {
    Phone,
    Pc,
    All,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ShellChoice {
    Bash,
    Zsh,
    Fish,
}

impl From<ShellChoice> for ShellType {
    fn from(value: ShellChoice) -> Self {
        match value {
            ShellChoice::Bash => Self::Bash,
            ShellChoice::Zsh => Self::Zsh,
            ShellChoice::Fish => Self::Fish,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum HookEvent {
    SessionOpen,
    CommandStart,
    PromptReady,
    SessionClose,
}

#[tokio::main]
async fn main() -> Result<()> {
    let command = Cli::parse().command;
    let paths = LocalPaths::discover()?;
    match command {
        Command::Daemon => daemon::run_daemon(paths).await,
        Command::SessionId => {
            println!("{}", Uuid::new_v4());
            Ok(())
        }
        Command::Shutdown => {
            ipc::request(
                &paths.socket_file(),
                &IpcRequest::new(IpcCommand::DaemonShutdown),
                CLI_TIMEOUT,
            )
            .await?;
            Ok(())
        }
        Command::Hook(command) => run_hook(command, &paths).await,
        Command::Pair { relay_url } => {
            pair(&http_client()?, relay_url, &paths.credentials_file()).await
        }
        Command::Install(command) => run_install(command).await,
        Command::Uninstall(command) => run_uninstall(command, &paths).await,
        Command::On(command) => arm(command, false, &paths).await,
        Command::Once(command) => arm(command, true, &paths).await,
        Command::Off => session_simple(IpcCommandFactory::Disarm, &paths).await,
        Command::Status => status(&paths).await,
        Command::Name { label } => name(label, &paths).await,
        Command::Ring { message, to } => ring(message, to, &paths).await,
        Command::Config => print_config(&paths),
        Command::Doctor => doctor(&http_client()?, &paths).await,
    }
}

fn http_client() -> Result<Client> {
    Ok(Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("shellbell/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

fn validate_relay_url(mut url: Url) -> Result<Url> {
    let localhost = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if url.scheme() != "https" && !(url.scheme() == "http" && localhost) {
        bail!("relay URL must use HTTPS (HTTP is allowed only for localhost)");
    }
    if url.cannot_be_a_base() || url.host_str().is_none() {
        bail!("relay URL must be an absolute HTTP URL");
    }
    url.set_query(None);
    url.set_fragment(None);
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

fn endpoint(base: &Url, path: &str) -> Result<Url> {
    base.join(path)
        .context("could not construct relay endpoint")
}

async fn pair(client: &Client, relay_url: Url, path: &Path) -> Result<()> {
    let relay_url = validate_relay_url(relay_url)?;
    let display_name = hostname::get()
        .context("could not determine machine name")?
        .to_string_lossy()
        .trim()
        .to_owned();
    let display_name = if display_name.is_empty() {
        "Shellbell source".into()
    } else {
        display_name
    };
    let response: PairingCreateResponse = send_json(
        client
            .post(endpoint(&relay_url, "api/pairings")?)
            .json(&PairingCreateRequest { display_name }),
    )
    .await
    .context("could not create pairing request")?;
    println!("Pairing code: {}", response.code);
    println!(
        "Approve this code in the Shellbell PWA before {}.",
        response.expires_at.to_rfc3339()
    );
    let deadline = Instant::now()
        + response
            .expires_at
            .signed_duration_since(chrono::Utc::now())
            .to_std()
            .unwrap_or_default();
    loop {
        if Instant::now() >= deadline {
            bail!("pairing request expired before it was approved");
        }
        sleep(Duration::from_secs(2)).await;
        let poll_url = endpoint(&relay_url, &format!("api/pairings/{}", response.id))?;
        let poll: PairingPollResponse = get_json_retry(client, poll_url, None)
            .await
            .context("could not poll pairing request")?;
        match poll.status {
            PairingState::Pending => continue,
            PairingState::Rejected => bail!("pairing request was rejected"),
            PairingState::Expired => bail!("pairing request expired"),
            PairingState::Approved => {
                let source_id = poll
                    .source_id
                    .context("approved pairing did not include a source identity")?;
                let source_token = poll
                    .source_token
                    .context("pairing token was already retrieved; create a new pairing request")?;
                save_credentials(
                    path,
                    &StoredCredentials {
                        relay_url,
                        source_id,
                        source_token,
                    },
                )?;
                println!(
                    "Paired successfully as source {source_id}. Credentials were stored securely."
                );
                return Ok(());
            }
        }
    }
}

async fn run_install(command: InstallCommand) -> Result<()> {
    let home = resolve_home(command.home)?;
    let real_home = resolve_home(None)?;
    let executable = env::current_exe()?
        .canonicalize()
        .unwrap_or(env::current_exe()?);
    let report = install::install(&InstallOptions {
        home: home.clone(),
        shells: command.shells.into_iter().map(Into::into).collect(),
        all_shells: command.all_shells,
        dry_run: command.dry_run,
        executable,
        manage_service: home == real_home,
    })?;
    for change in report.changes {
        println!("Changed: {change}");
    }
    if let Some(kind) = report.service_kind {
        println!("Daemon startup: {kind}");
    }
    println!("Sessions remain off until `shellbell on` or `shellbell once`.");
    Ok(())
}

async fn run_uninstall(command: UninstallCommand, paths: &LocalPaths) -> Result<()> {
    let home = resolve_home(command.home)?;
    let real_home = resolve_home(None)?;
    let manages_current = home == real_home && !command.dry_run;
    if manages_current && let Ok(session_id) = current_session_id() {
        let _ = send_explicit(paths, IpcCommand::Disarm { session_id }).await;
    }
    let report = install::uninstall(&home, command.dry_run, command.purge, home == real_home)?;
    if manages_current {
        let _ = ipc::request(
            &paths.socket_file(),
            &IpcRequest::new(IpcCommand::DaemonShutdown),
            Duration::from_millis(300),
        )
        .await;
    }
    for change in report.changes {
        println!("Changed: {change}");
    }
    if !command.purge {
        println!("Preserved configuration, local state, and paired source credentials.");
    }
    Ok(())
}

fn resolve_home(value: Option<PathBuf>) -> Result<PathBuf> {
    match value {
        Some(path) if path.is_absolute() => Ok(path),
        Some(_) => bail!("--home must be an absolute path"),
        None => env::var_os("HOME")
            .map(PathBuf::from)
            .context("HOME is not set"),
    }
}

async fn arm(command: ArmCommand, once: bool, paths: &LocalPaths) -> Result<()> {
    let session_id = current_session_id()?;
    ensure_daemon(paths, false).await?;
    ensure_current_session_registered(paths).await?;
    let config = EffectiveConfig::load(&paths.config_file())?;
    let minimum_active = command
        .minimum_active
        .as_deref()
        .map(|value| parse_session_duration("--after", value))
        .transpose()?
        .unwrap_or(config.minimum_active);
    let idle_for = command
        .idle_for
        .as_deref()
        .map(|value| parse_session_duration("--idle", value))
        .transpose()?
        .unwrap_or(config.idle_for);
    let targets = command
        .to
        .map(targets)
        .unwrap_or_else(|| config.targets.clone());
    let fields = (
        session_id,
        duration_ms(minimum_active),
        duration_ms(idle_for),
        targets,
    );
    let request = if once {
        IpcCommand::ArmOnce {
            session_id: fields.0,
            minimum_active_ms: fields.1,
            idle_for_ms: fields.2,
            targets: fields.3,
        }
    } else {
        IpcCommand::ArmPersistent {
            session_id: fields.0,
            minimum_active_ms: fields.1,
            idle_for_ms: fields.2,
            targets: fields.3,
        }
    };
    send_explicit(paths, request).await?;
    println!(
        "Session {session_id} armed {} (after {}, idle {}).",
        if once { "once" } else { "persistently" },
        humantime::format_duration(minimum_active),
        humantime::format_duration(idle_for)
    );
    println!(
        "Monitoring is active now; a following command in this same submission or from the next prompt will be measured."
    );
    Ok(())
}

enum IpcCommandFactory {
    Disarm,
}

async fn session_simple(factory: IpcCommandFactory, paths: &LocalPaths) -> Result<()> {
    let session_id = current_session_id()?;
    ensure_daemon(paths, false).await?;
    ensure_current_session_registered(paths).await?;
    let command = match factory {
        IpcCommandFactory::Disarm => IpcCommand::Disarm { session_id },
    };
    send_explicit(paths, command).await?;
    println!("Session {session_id} disarmed.");
    Ok(())
}

async fn name(label: Option<String>, paths: &LocalPaths) -> Result<()> {
    let session_id = current_session_id()?;
    let label = label
        .map(|label| shellbell_protocol::validate_name(&label).map_err(anyhow::Error::msg))
        .transpose()?;
    ensure_daemon(paths, false).await?;
    ensure_current_session_registered(paths).await?;
    send_explicit(
        paths,
        IpcCommand::SetLabel {
            session_id,
            label: label.clone(),
        },
    )
    .await?;
    match label {
        Some(label) => println!("Session {session_id} named {label:?}."),
        None => println!("Session {session_id} label cleared."),
    }
    Ok(())
}

async fn status(paths: &LocalPaths) -> Result<()> {
    let session_id = current_session_id()?;
    ensure_daemon(paths, false).await?;
    ensure_current_session_registered(paths).await?;
    let response = send_explicit(paths, IpcCommand::Status { session_id }).await?;
    let Some(IpcResult::Status { session }) = response.result else {
        bail!("daemon returned an unexpected status response");
    };
    println!("Session ID: {}", session.session_id);
    println!("Shell: {}", session.shell_type);
    println!("Mode: {}", session.mode);
    println!(
        "State: {}",
        if session.burst_notified {
            "notified".to_owned()
        } else {
            session.state.to_string()
        }
    );
    println!(
        "Accumulated active: {}",
        humantime::format_duration(Duration::from_millis(session.accumulated_active_ms))
    );
    println!(
        "Settling deadline: {}",
        session
            .settling_deadline_wall_ms
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|value| value.to_rfc3339())
            .unwrap_or_else(|| "none".into())
    );
    println!("Label: {}", session.label.as_deref().unwrap_or("none"));
    println!(
        "Delivery targets: {}",
        if session.targets.is_empty() {
            "all".into()
        } else {
            session.targets.join(", ")
        }
    );
    println!("Daemon connection: connected");
    Ok(())
}

async fn ring(message: Option<String>, target: Target, paths: &LocalPaths) -> Result<()> {
    let message =
        shellbell_protocol::validate_message(message.as_deref()).map_err(anyhow::Error::msg)?;
    ensure_daemon(paths, false).await?;
    if current_session_id().is_ok() {
        ensure_current_session_registered(paths).await?;
    }
    let response = send_explicit(
        paths,
        IpcCommand::ManualRing {
            session_id: current_session_id().ok(),
            message,
            targets: targets(target),
        },
    )
    .await?;
    let Some(IpcResult::RingQueued { event_id }) = response.result else {
        bail!("daemon returned an unexpected manual-ring response");
    };
    if let Some(event_id) = event_id {
        println!("Ring queued as event {event_id}.");
    } else {
        println!("Current activity burst has already rung; duplicate suppressed.");
    }
    Ok(())
}

fn targets(target: Target) -> Vec<String> {
    match target {
        Target::Phone => vec!["phone".into()],
        Target::Pc => vec!["pc".into()],
        Target::All => vec![],
    }
}

fn print_config(paths: &LocalPaths) -> Result<()> {
    let config = EffectiveConfig::load(&paths.config_file())?;
    println!("Config path: {}", paths.config_file().display());
    println!(
        "Credentials path: {} ({})",
        paths.credentials_file().display(),
        if paths.credentials_file().exists() {
            "paired credentials present"
        } else {
            "not paired"
        }
    );
    println!(
        "activity.minimum_active = {:?}",
        humantime::format_duration(config.minimum_active).to_string()
    );
    println!(
        "activity.idle_for = {:?}",
        humantime::format_duration(config.idle_for).to_string()
    );
    println!("delivery.targets = {:?}", config.targets);
    println!("display.automatic_message = {:?}", config.automatic_message);
    println!("daemon.queue_limit = {}", config.queue_limit);
    println!(
        "daemon.event_max_age = {:?}",
        humantime::format_duration(config.event_max_age).to_string()
    );
    Ok(())
}

async fn run_hook(command: HookCommand, paths: &LocalPaths) -> Result<()> {
    let session_id = current_session_id()?;
    let request = match command.event {
        HookEvent::SessionOpen => IpcCommand::SessionOpen {
            session_id,
            shell_pid: command.pid.context("session-open hook requires --pid")?,
            shell_type: command
                .shell
                .context("session-open hook requires --shell")?
                .into(),
            tty: command
                .tty
                .filter(|value| !value.is_empty() && value != "not a tty"),
        },
        HookEvent::CommandStart => IpcCommand::CommandStart { session_id },
        HookEvent::PromptReady => IpcCommand::PromptReady { session_id },
        HookEvent::SessionClose => IpcCommand::SessionClose { session_id },
    };
    if ipc::request(
        &paths.socket_file(),
        &IpcRequest::new(request.clone()),
        HOOK_TIMEOUT,
    )
    .await
    .is_ok()
    {
        return Ok(());
    }
    ensure_daemon(paths, true).await?;
    if !matches!(request, IpcCommand::SessionOpen { .. }) {
        let _ = ensure_current_session_registered_with_timeout(paths, HOOK_TIMEOUT).await;
    }
    let _ = ipc::request(
        &paths.socket_file(),
        &IpcRequest::new(request),
        HOOK_TIMEOUT,
    )
    .await;
    Ok(())
}

async fn ensure_daemon(paths: &LocalPaths, hook: bool) -> Result<()> {
    let timeout_duration = if hook {
        HOOK_TIMEOUT
    } else {
        Duration::from_millis(200)
    };
    if ipc::request(
        &paths.socket_file(),
        &IpcRequest::new(IpcCommand::DaemonPing),
        timeout_duration,
    )
    .await
    .is_ok()
    {
        return Ok(());
    }
    paths.ensure_private_dirs()?;
    let executable = env::current_exe().context("could not find the shellbell executable")?;
    let mut process = ProcessCommand::new(executable);
    process
        .arg("__daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    process.process_group(0);
    process
        .spawn()
        .context("could not start the local Shellbell daemon")?;
    let attempts = if hook { 8 } else { 50 };
    for _ in 0..attempts {
        sleep(Duration::from_millis(15)).await;
        if ipc::request(
            &paths.socket_file(),
            &IpcRequest::new(IpcCommand::DaemonPing),
            timeout_duration,
        )
        .await
        .is_ok()
        {
            return Ok(());
        }
    }
    if hook {
        bail!("local daemon is starting")
    } else {
        bail!("local Shellbell daemon did not become ready")
    }
}

async fn ensure_current_session_registered(paths: &LocalPaths) -> Result<()> {
    ensure_current_session_registered_with_timeout(paths, CLI_TIMEOUT).await
}

async fn ensure_current_session_registered_with_timeout(
    paths: &LocalPaths,
    operation_timeout: Duration,
) -> Result<()> {
    let (session_id, shell_pid, shell_type, tty) = current_session_metadata()?;
    ipc::request(
        &paths.socket_file(),
        &IpcRequest::new(IpcCommand::SessionOpen {
            session_id,
            shell_pid,
            shell_type,
            tty,
        }),
        operation_timeout,
    )
    .await?;
    Ok(())
}

fn current_session_metadata() -> Result<(Uuid, u32, ShellType, Option<String>)> {
    let session_id = current_session_id()?;
    let shell_pid = env::var("SHELLBELL_SHELL_PID")
        .context("SHELLBELL_SHELL_PID is missing; start a new shell after `shellbell install`")?
        .parse()
        .context("SHELLBELL_SHELL_PID is invalid")?;
    let shell_type = match env::var("SHELLBELL_SHELL_TYPE").as_deref() {
        Ok("bash") => ShellType::Bash,
        Ok("zsh") => ShellType::Zsh,
        Ok("fish") => ShellType::Fish,
        _ => bail!("SHELLBELL_SHELL_TYPE is invalid; start a new integrated shell"),
    };
    let tty = env::var("SHELLBELL_TTY")
        .ok()
        .filter(|value| !value.is_empty() && value != "not a tty");
    Ok((session_id, shell_pid, shell_type, tty))
}

fn current_session_id() -> Result<Uuid> {
    let value = env::var("SHELLBELL_SESSION_ID")
        .context("this shell is not integrated; run `shellbell install` and start a new shell")?;
    Uuid::parse_str(&value).context("SHELLBELL_SESSION_ID is invalid")
}

async fn send_explicit(paths: &LocalPaths, command: IpcCommand) -> Result<ipc::IpcResponse> {
    ipc::request(&paths.socket_file(), &IpcRequest::new(command), CLI_TIMEOUT).await
}

async fn doctor(client: &Client, paths: &LocalPaths) -> Result<()> {
    let mut failures = 0_u32;
    let mut warnings = 0_u32;
    match EffectiveConfig::load(&paths.config_file()) {
        Ok(_) => {
            doctor_line(
                "OK",
                &format!("config parses ({})", paths.config_file().display()),
            );
        }
        Err(error) => {
            doctor_line("FAILURE", &format!("config: {error}"));
            failures += 1;
        }
    }
    let credentials = match load_credentials(&paths.credentials_file()) {
        Ok(credentials) => {
            doctor_line("OK", "source is paired");
            #[cfg(unix)]
            {
                match fs::metadata(paths.credentials_file()) {
                    Ok(metadata) if metadata.permissions().mode() & 0o077 == 0 => {
                        doctor_line("OK", "credentials permissions are private")
                    }
                    Ok(_) => {
                        doctor_line(
                            "FAILURE",
                            "credentials are readable by group or other users",
                        );
                        failures += 1;
                    }
                    Err(error) => {
                        doctor_line("FAILURE", &format!("credentials metadata: {error}"));
                        failures += 1;
                    }
                }
            }
            Some(credentials)
        }
        Err(error) => {
            doctor_line("FAILURE", &error.to_string());
            failures += 1;
            None
        }
    };
    if let Some(credentials) = credentials {
        match get_json_retry::<HealthResponse>(
            client,
            endpoint(&credentials.relay_url, "health")?,
            None,
        )
        .await
        {
            Ok(_) => doctor_line("OK", "relay connectivity"),
            Err(error) => {
                doctor_line("FAILURE", &format!("relay connectivity: {error}"));
                failures += 1;
            }
        }
        match get_json_retry::<SourceView>(
            client,
            endpoint(&credentials.relay_url, "api/sources/self")?,
            Some(&credentials.source_token),
        )
        .await
        {
            Ok(source) => doctor_line(
                "OK",
                &format!("source token valid for {}", source.display_name),
            ),
            Err(error) => {
                doctor_line("FAILURE", &format!("source token validation: {error}"));
                failures += 1;
            }
        }
    }
    match daemon::ping(paths).await {
        Ok(status) => {
            doctor_line("OK", "daemon process, IPC socket, and round trip");
            doctor_line(
                "OK",
                &format!(
                    "durable queue: {} pending, {} dropped by policy",
                    status.pending_events, status.dropped_events
                ),
            );
            if status.authorization_failures > 0 {
                doctor_line(
                    "FAILURE",
                    &format!(
                        "durable queue has {} authorization failure(s); pair again",
                        status.authorization_failures
                    ),
                );
                failures += 1;
            }
        }
        Err(error) => {
            doctor_line("FAILURE", &format!("daemon/IPC unavailable: {error}"));
            failures += 1;
        }
    }
    if install::systemd_user_available() {
        let active = install::systemctl_status(
            ["is-active", "--quiet", "shellbell.service"],
            Duration::from_secs(2),
        );
        if active {
            doctor_line("OK", "systemd user service is active");
        } else {
            doctor_line(
                "WARNING",
                "systemd user manager is available but shellbell.service is not active",
            );
            warnings += 1;
        }
    } else {
        doctor_line(
            "WARNING",
            "systemd user manager unavailable; shell-start lazy activation is used",
        );
        warnings += 1;
    }
    inspect_hooks(&resolve_home(None)?, &mut failures, &mut warnings)?;
    if let Ok(session_id) = current_session_id() {
        match send_explicit(paths, IpcCommand::Status { session_id }).await {
            Ok(_) => doctor_line("OK", "current shell session is registered"),
            Err(error) => {
                doctor_line("WARNING", &format!("current shell session: {error}"));
                warnings += 1;
            }
        }
    } else {
        doctor_line(
            "WARNING",
            "current process is not an integrated interactive shell",
        );
        warnings += 1;
    }
    println!("Summary: {failures} failure(s), {warnings} warning(s)");
    if failures > 0 {
        bail!("Shellbell doctor found {failures} failure(s)");
    }
    Ok(())
}

fn inspect_hooks(home: &Path, failures: &mut u32, warnings: &mut u32) -> Result<()> {
    for (shell, startup, hook) in [
        (
            "bash",
            home.join(".bashrc"),
            home.join(".config/shellbell/hooks/bash.sh"),
        ),
        (
            "zsh",
            home.join(".zshrc"),
            home.join(".config/shellbell/hooks/zsh.sh"),
        ),
        (
            "fish",
            home.join(".config/fish/config.fish"),
            home.join(".config/shellbell/hooks/fish.fish"),
        ),
    ] {
        let entries = fs::read_to_string(&startup)
            .ok()
            .map(|value| {
                value
                    .lines()
                    .filter(|line| line.contains(STARTUP_MARKER))
                    .count()
            })
            .unwrap_or_default();
        match entries {
            1 if hook.is_file() => doctor_line(
                "OK",
                &format!("{shell} managed hook installed and readable"),
            ),
            1 => {
                doctor_line(
                    "FAILURE",
                    &format!("{shell} startup entry exists but hook file is missing"),
                );
                *failures += 1;
            }
            0 => {
                doctor_line("WARNING", &format!("{shell} integration is not installed"));
                *warnings += 1;
            }
            count => {
                doctor_line(
                    "FAILURE",
                    &format!("{shell} startup file has {count} duplicate managed entries"),
                );
                *failures += 1;
            }
        }
    }
    Ok(())
}

fn doctor_line(level: &str, message: &str) {
    println!("{level}: {message}");
}

async fn send_json<T: DeserializeOwned>(request: reqwest::RequestBuilder) -> Result<T> {
    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    Ok(response.json().await?)
}

async fn get_json_retry<T: DeserializeOwned>(
    client: &Client,
    url: Url,
    bearer: Option<&str>,
) -> Result<T> {
    let mut last_error = None;
    for attempt in 0..3 {
        let mut request = client.get(url.clone());
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        match request.send().await {
            Ok(response) if response.status().is_success() => return Ok(response.json().await?),
            Ok(response) if response.status().is_server_error() && attempt < 2 => {
                last_error = Some(response_error(response).await)
            }
            Ok(response) => return Err(response_error(response).await),
            Err(error) if (error.is_timeout() || error.is_connect()) && attempt < 2 => {
                last_error = Some(error.into())
            }
            Err(error) => return Err(error.into()),
        }
        sleep(Duration::from_millis(250 * (attempt + 1))).await;
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("request failed")))
}

async fn response_error(response: reqwest::Response) -> anyhow::Error {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if let Ok(error) = serde_json::from_str::<shellbell_protocol::ApiError>(&body) {
        anyhow::anyhow!("{}: {}", error.error.code, error.error.message)
    } else if status == StatusCode::UNAUTHORIZED {
        anyhow::anyhow!("unauthorized; run `shellbell pair <relay-url>` again")
    } else {
        anyhow::anyhow!("relay returned HTTP {status}")
    }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_or_local_http_is_allowed() {
        assert!(validate_relay_url(Url::parse("https://relay.example").unwrap()).is_ok());
        assert!(validate_relay_url(Url::parse("http://localhost:8080").unwrap()).is_ok());
        assert!(validate_relay_url(Url::parse("http://relay.example").unwrap()).is_err());
    }

    #[test]
    fn stored_credentials_round_trip_without_displaying_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let config = StoredCredentials {
            relay_url: Url::parse("https://relay.example/").unwrap(),
            source_id: Uuid::new_v4(),
            source_token: "sb_src_secret".into(),
        };
        save_credentials(&path, &config).unwrap();
        assert_eq!(load_credentials(&path).unwrap(), config);
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn cli_contract_includes_all_milestone_commands() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
        let names: Vec<_> = Cli::command()
            .get_subcommands()
            .map(|command| command.get_name().to_owned())
            .collect();
        for required in [
            "pair",
            "install",
            "uninstall",
            "on",
            "once",
            "off",
            "status",
            "name",
            "ring",
            "config",
            "doctor",
        ] {
            assert!(names.contains(&required.to_owned()));
        }
    }
}
