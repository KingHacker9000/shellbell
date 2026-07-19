use crate::{
    ShellType,
    config::{ensure_default_config, ensure_private_dir},
};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use std::{
    env, fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use uuid::Uuid;

pub const STARTUP_MARKER: &str = "# shellbell managed integration";
const SERVICE_MARKER: &str = "# Managed by Shellbell";

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub home: PathBuf,
    pub shells: Vec<ShellType>,
    pub all_shells: bool,
    pub dry_run: bool,
    pub executable: PathBuf,
    pub manage_service: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeReport {
    pub changes: Vec<String>,
    pub service_kind: Option<ServiceKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceKind {
    Systemd,
    LazyShellStart,
}

impl std::fmt::Display for ServiceKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Systemd => "systemd user service",
            Self::LazyShellStart => "shell-start lazy activation",
        })
    }
}

pub fn install(options: &InstallOptions) -> Result<ChangeReport> {
    validate_home(&options.home)?;
    let mut report = ChangeReport::default();
    let config_dir = options.home.join(".config/shellbell");
    let hook_dir = config_dir.join("hooks");
    let config_file = config_dir.join("config.toml");
    let shells = selected_shells(options);
    if shells.is_empty() {
        bail!("no supported shell was selected or detected");
    }

    if !config_file.exists() {
        report
            .changes
            .push(format!("create {}", config_file.display()));
        if !options.dry_run {
            ensure_default_config(&config_file)?;
        }
    } else {
        crate::EffectiveConfig::load(&config_file)?;
    }
    if !options.dry_run {
        ensure_private_dir(&hook_dir)?;
    }

    for shell in shells {
        let hook = hook_dir.join(format!("{shell}.{}", hook_extension(shell)));
        let content = hook_content(shell);
        if fs::read_to_string(&hook).ok().as_deref() != Some(content) {
            report.changes.push(format!("write {}", hook.display()));
            if !options.dry_run {
                atomic_managed_write(&hook, content.as_bytes(), 0o600)?;
            }
        }
        let startup = startup_file(&options.home, shell);
        let line = source_line(shell, &hook);
        if !startup_contains_line(&startup, &line)? {
            if startup.exists() {
                report
                    .changes
                    .push(format!("back up {}", startup.display()));
            }
            report
                .changes
                .push(format!("add managed source line to {}", startup.display()));
            if !options.dry_run {
                backup_if_present(&startup)?;
                append_line_preserving_format(&startup, &line)?;
            }
        }
    }

    let osrelease = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let systemd = options.manage_service && systemd_user_available();
    let service_kind = service_kind(&osrelease, systemd);
    report.service_kind = Some(service_kind);
    if service_kind == ServiceKind::Systemd {
        let unit = options.home.join(".config/systemd/user/shellbell.service");
        let content = systemd_unit(&options.executable);
        if fs::read_to_string(&unit).ok().as_deref() != Some(content.as_str()) {
            report.changes.push(format!("write {}", unit.display()));
            if !options.dry_run {
                let parent = unit.parent().context("service unit path has no parent")?;
                fs::create_dir_all(parent)?;
                atomic_managed_write(&unit, content.as_bytes(), 0o600)?;
            }
        }
        if !options.dry_run {
            run_systemctl(["daemon-reload"])?;
            run_systemctl(["enable", "--now", "shellbell.service"])?;
            report
                .changes
                .push("enable and start shellbell.service".into());
        } else {
            report
                .changes
                .push("would enable and start shellbell.service".into());
        }
    } else {
        report
            .changes
            .push("use shell-start lazy daemon activation (no usable systemd user manager)".into());
    }
    Ok(report)
}

pub fn uninstall(
    home: &Path,
    dry_run: bool,
    purge: bool,
    manage_service: bool,
) -> Result<ChangeReport> {
    validate_home(home)?;
    let mut report = ChangeReport::default();
    let unit = home.join(".config/systemd/user/shellbell.service");
    if unit.exists() && is_owned_service(&unit)? {
        if manage_service && systemd_user_available() && !dry_run {
            let _ = run_systemctl(["disable", "--now", "shellbell.service"]);
        }
        report.changes.push(format!("remove {}", unit.display()));
        if !dry_run {
            fs::remove_file(&unit)?;
            if manage_service && systemd_user_available() {
                let _ = run_systemctl(["daemon-reload"]);
            }
        }
    }
    for shell in [ShellType::Bash, ShellType::Zsh, ShellType::Fish] {
        let startup = startup_file(home, shell);
        if startup_has_marker(&startup)? {
            report
                .changes
                .push(format!("back up {}", startup.display()));
            report.changes.push(format!(
                "remove managed source line from {}",
                startup.display()
            ));
            if !dry_run {
                backup_if_present(&startup)?;
                remove_managed_lines(&startup)?;
            }
        }
        let hook = home
            .join(".config/shellbell/hooks")
            .join(format!("{shell}.{}", hook_extension(shell)));
        if hook.exists() {
            report.changes.push(format!("remove {}", hook.display()));
            if !dry_run {
                fs::remove_file(hook)?;
            }
        }
    }
    if purge {
        for path in [
            home.join(".config/shellbell"),
            home.join(".local/share/shellbell"),
        ] {
            if path.exists() {
                report.changes.push(format!("remove {}", path.display()));
                if !dry_run {
                    fs::remove_dir_all(path)?;
                }
            }
        }
    }
    if report.changes.is_empty() {
        report.changes.push("no managed files found".into());
    }
    Ok(report)
}

fn selected_shells(options: &InstallOptions) -> Vec<ShellType> {
    let mut selected = if options.all_shells {
        vec![ShellType::Bash, ShellType::Zsh, ShellType::Fish]
    } else if !options.shells.is_empty() {
        options.shells.clone()
    } else {
        [ShellType::Bash, ShellType::Zsh, ShellType::Fish]
            .into_iter()
            .filter(|shell| executable_on_path(&shell.to_string()))
            .collect()
    };
    selected.sort_by_key(|shell| shell.to_string());
    selected.dedup();
    selected
}

fn executable_on_path(name: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|directory| {
            let candidate = directory.join(name);
            fs::metadata(candidate).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
    })
}

fn validate_home(home: &Path) -> Result<()> {
    if !home.is_absolute() {
        bail!("--home must be an absolute path");
    }
    Ok(())
}

fn hook_extension(shell: ShellType) -> &'static str {
    match shell {
        ShellType::Fish => "fish",
        ShellType::Bash | ShellType::Zsh => "sh",
    }
}

fn startup_file(home: &Path, shell: ShellType) -> PathBuf {
    match shell {
        ShellType::Bash => home.join(".bashrc"),
        ShellType::Zsh => home.join(".zshrc"),
        ShellType::Fish => home.join(".config/fish/config.fish"),
    }
}

fn source_line(shell: ShellType, hook: &Path) -> String {
    let path = shell_quote(hook);
    match shell {
        ShellType::Fish => format!("source {path} {STARTUP_MARKER}"),
        ShellType::Bash | ShellType::Zsh => format!("source {path} {STARTUP_MARKER}"),
    }
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn startup_contains_line(path: &Path, expected: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    Ok(fs::read_to_string(path)?
        .lines()
        .any(|line| line == expected))
}

fn startup_has_marker(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    Ok(fs::read_to_string(path)?
        .lines()
        .any(|line| line.contains(STARTUP_MARKER)))
}

fn backup_if_present(path: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("startup");
    let backup = path.with_file_name(format!(
        "{name}.shellbell-backup-{}-{}",
        Utc::now().format("%Y%m%d%H%M%S"),
        &Uuid::new_v4().simple().to_string()[..8]
    ));
    fs::copy(path, &backup)?;
    fs::set_permissions(&backup, fs::metadata(path)?.permissions())?;
    Ok(Some(backup))
}

fn append_line_preserving_format(path: &Path, line: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let existing = fs::read(path).unwrap_or_default();
    let newline = if existing.windows(2).any(|bytes| bytes == b"\r\n") {
        b"\r\n".as_slice()
    } else {
        b"\n".as_slice()
    };
    let existed = path.exists();
    let original_mode = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions().mode() & 0o777)
        .unwrap_or(0o644);
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true).mode(original_mode);
    let mut file = options.open(path)?;
    if !existing.is_empty() && !existing.ends_with(b"\n") {
        file.write_all(newline)?;
    }
    file.write_all(line.as_bytes())?;
    file.write_all(newline)?;
    file.sync_all()?;
    if existed {
        fs::set_permissions(path, fs::Permissions::from_mode(original_mode))?;
    }
    Ok(())
}

fn remove_managed_lines(path: &Path) -> Result<()> {
    let bytes = fs::read(path)?;
    let mode = fs::metadata(path)?.permissions().mode() & 0o777;
    let newline = if bytes.windows(2).any(|value| value == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let text = String::from_utf8(bytes).context("startup file is not valid UTF-8")?;
    let had_final_newline = text.ends_with('\n');
    let retained: Vec<&str> = text
        .lines()
        .filter(|line| !line.contains(STARTUP_MARKER))
        .collect();
    let mut output = retained.join(newline);
    if had_final_newline && !output.is_empty() {
        output.push_str(newline);
    }
    atomic_managed_write(path, output.as_bytes(), mode)
}

fn atomic_managed_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let parent = path.parent().context("managed file path has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".shellbell-{}.tmp", Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub fn is_wsl(osrelease: &str) -> bool {
    let normalized = osrelease.to_ascii_lowercase();
    normalized.contains("microsoft") || normalized.contains("wsl")
}

pub fn service_kind(osrelease: &str, systemd_available: bool) -> ServiceKind {
    let _wsl = is_wsl(osrelease);
    if systemd_available {
        ServiceKind::Systemd
    } else {
        ServiceKind::LazyShellStart
    }
}

pub fn systemd_user_available() -> bool {
    systemctl_status(["show-environment"], std::time::Duration::from_millis(750))
}

fn run_systemctl<const N: usize>(arguments: [&str; N]) -> Result<()> {
    if !systemctl_status(arguments, std::time::Duration::from_secs(10)) {
        bail!("systemctl --user operation failed");
    }
    Ok(())
}

pub fn systemctl_status<const N: usize>(
    arguments: [&str; N],
    operation_timeout: std::time::Duration,
) -> bool {
    let mut child = match Command::new("systemctl")
        .arg("--user")
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if started.elapsed() < operation_timeout => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

pub fn systemd_unit(executable: &Path) -> String {
    format!(
        "{SERVICE_MARKER}\n[Unit]\nDescription=Shellbell per-user activity daemon\nAfter=network-online.target\nWants=network-online.target\nStartLimitIntervalSec=60\nStartLimitBurst=5\n\n[Service]\nType=simple\nExecStart={} __daemon\nRestart=on-failure\nRestartSec=5s\nNoNewPrivileges=true\n\n[Install]\nWantedBy=default.target\n",
        systemd_quote(executable)
    )
}

fn systemd_quote(path: &Path) -> String {
    let escaped = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn is_owned_service(path: &Path) -> Result<bool> {
    Ok(fs::read_to_string(path)?.lines().next() == Some(SERVICE_MARKER))
}

pub fn hook_content(shell: ShellType) -> &'static str {
    match shell {
        ShellType::Bash => BASH_HOOK,
        ShellType::Zsh => ZSH_HOOK,
        ShellType::Fish => FISH_HOOK,
    }
}

const BASH_HOOK: &str = r#"# Managed by Shellbell. Edits are replaced by `shellbell install`.
if [[ $- == *i* ]] && [[ ${SHELLBELL_SHELL_PID:-} != "$$" ]]; then
  export SHELLBELL_SESSION_ID="$(command cat /proc/sys/kernel/random/uuid 2>/dev/null || command shellbell __session-id 2>/dev/null)"
  export SHELLBELL_SHELL_PID="$$"
  export SHELLBELL_SHELL_TYPE="bash"
  export SHELLBELL_TTY="$(command tty 2>/dev/null || :)"
  __shellbell_guard=1
  __shellbell_at_prompt=0
  __shellbell_saved_debug="$(trap -p DEBUG)"
  __shellbell_saved_exit="$(trap -p EXIT)"

  __shellbell_run_saved_trap() {
    local spec="$1" event="$2" body quoted
    [[ -z $spec ]] && return 0
    quoted="${spec#trap -- }"
    quoted="${quoted% $event}"
    builtin eval "body=$quoted"
    builtin eval -- "$body"
  }

  __shellbell_debug() {
    if [[ $__shellbell_guard == 0 && $__shellbell_at_prompt == 1 ]]; then
      __shellbell_guard=1
      __shellbell_at_prompt=0
      command shellbell __hook command-start >/dev/null 2>&1 || :
      __shellbell_guard=0
    fi
    __shellbell_run_saved_trap "$__shellbell_saved_debug" DEBUG
    return $?
  }

  __shellbell_precmd() {
    local previous_status=$?
    __shellbell_guard=1
    command shellbell __hook prompt-ready >/dev/null 2>&1 || :
    __shellbell_at_prompt=1
    __shellbell_guard=0
    return "$previous_status"
  }

  __shellbell_exit() {
    local previous_status=$?
    __shellbell_guard=1
    command shellbell __hook session-close >/dev/null 2>&1 || :
    __shellbell_run_saved_trap "$__shellbell_saved_exit" EXIT
    return "$previous_status"
  }

  command shellbell __hook session-open --shell bash --pid "$$" --tty "$SHELLBELL_TTY" >/dev/null 2>&1 || :
  if declare -p PROMPT_COMMAND 2>/dev/null | command grep -q '^declare -a'; then
    [[ " ${PROMPT_COMMAND[*]} " == *" __shellbell_precmd "* ]] || PROMPT_COMMAND+=(__shellbell_precmd)
  else
    [[ ${PROMPT_COMMAND:-} == *"__shellbell_precmd"* ]] || PROMPT_COMMAND="${PROMPT_COMMAND:+$PROMPT_COMMAND; }__shellbell_precmd"
  fi
  trap '__shellbell_debug' DEBUG
  trap '__shellbell_exit' EXIT
  __shellbell_guard=0
fi
"#;

const ZSH_HOOK: &str = r#"# Managed by Shellbell. Edits are replaced by `shellbell install`.
if [[ -o interactive && ${SHELLBELL_SHELL_PID:-} != "$$" ]]; then
  export SHELLBELL_SESSION_ID="$(command cat /proc/sys/kernel/random/uuid 2>/dev/null || command shellbell __session-id 2>/dev/null)"
  export SHELLBELL_SHELL_PID="$$"
  export SHELLBELL_SHELL_TYPE="zsh"
  export SHELLBELL_TTY="${TTY:-}"
  typeset -g __shellbell_guard=1
  autoload -Uz add-zsh-hook
  __shellbell_zsh_preexec() {
    (( __shellbell_guard )) && return 0
    __shellbell_guard=1
    command shellbell __hook command-start >/dev/null 2>&1 || true
    __shellbell_guard=0
  }
  __shellbell_zsh_precmd() {
    __shellbell_guard=1
    command shellbell __hook prompt-ready >/dev/null 2>&1 || true
    __shellbell_guard=0
  }
  __shellbell_zsh_exit() {
    __shellbell_guard=1
    command shellbell __hook session-close >/dev/null 2>&1 || true
  }
  add-zsh-hook -D preexec __shellbell_zsh_preexec 2>/dev/null || true
  add-zsh-hook -D precmd __shellbell_zsh_precmd 2>/dev/null || true
  add-zsh-hook -D zshexit __shellbell_zsh_exit 2>/dev/null || true
  add-zsh-hook preexec __shellbell_zsh_preexec
  add-zsh-hook precmd __shellbell_zsh_precmd
  add-zsh-hook zshexit __shellbell_zsh_exit
  command shellbell __hook session-open --shell zsh --pid "$$" --tty "$SHELLBELL_TTY" >/dev/null 2>&1 || true
  __shellbell_guard=0
fi
"#;

const FISH_HOOK: &str = r#"# Managed by Shellbell. Edits are replaced by `shellbell install`.
if status is-interactive; and test "$SHELLBELL_SHELL_PID" != "$fish_pid"
    set -gx SHELLBELL_SESSION_ID (command cat /proc/sys/kernel/random/uuid 2>/dev/null; or command shellbell __session-id 2>/dev/null)
    set -gx SHELLBELL_SHELL_PID $fish_pid
    set -gx SHELLBELL_SHELL_TYPE fish
    set -gx SHELLBELL_TTY (command tty 2>/dev/null; or echo '')
    set -g __shellbell_guard 1
    function __shellbell_fish_preexec --on-event fish_preexec
        test "$__shellbell_guard" = 1; and return
        set -g __shellbell_guard 1
        command shellbell __hook command-start >/dev/null 2>&1; or true
        set -g __shellbell_guard 0
    end
    function __shellbell_fish_postexec --on-event fish_postexec
        set -g __shellbell_guard 1
        command shellbell __hook prompt-ready >/dev/null 2>&1; or true
        set -g __shellbell_guard 0
    end
    function __shellbell_fish_exit --on-event fish_exit
        set -g __shellbell_guard 1
        command shellbell __hook session-close >/dev/null 2>&1; or true
    end
    command shellbell __hook session-open --shell fish --pid $fish_pid --tty "$SHELLBELL_TTY" >/dev/null 2>&1; or true
    set -g __shellbell_guard 0
end
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn options(home: &Path) -> InstallOptions {
        InstallOptions {
            home: home.to_owned(),
            shells: vec![ShellType::Bash, ShellType::Zsh, ShellType::Fish],
            all_shells: false,
            dry_run: false,
            executable: PathBuf::from("/usr/bin/shellbell"),
            manage_service: false,
        }
    }

    #[test]
    fn install_is_idempotent_and_uninstall_preserves_unrelated_content() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        fs::write(home.join(".bashrc"), "export KEEP=yes\r\n").unwrap();
        let first = install(&options(home)).unwrap();
        assert!(
            first
                .changes
                .iter()
                .any(|change| change.contains(".bashrc"))
        );
        let installed = fs::read(home.join(".bashrc")).unwrap();
        assert!(installed.windows(2).any(|value| value == b"\r\n"));
        let second = install(&options(home)).unwrap();
        assert_eq!(
            second
                .changes
                .iter()
                .filter(|change| change.contains("source line"))
                .count(),
            0
        );
        uninstall(home, false, false, false).unwrap();
        assert_eq!(
            fs::read_to_string(home.join(".bashrc")).unwrap(),
            "export KEEP=yes\r\n"
        );
        assert!(home.join(".config/shellbell/config.toml").exists());
        let again = uninstall(home, false, false, false).unwrap();
        assert_eq!(again.changes, vec!["no managed files found"]);
    }

    #[test]
    fn install_creates_backups_and_preserves_permissions() {
        let directory = tempfile::tempdir().unwrap();
        let startup = directory.path().join(".zshrc");
        fs::write(&startup, "setopt promptsubst\n").unwrap();
        fs::set_permissions(&startup, fs::Permissions::from_mode(0o640)).unwrap();
        install(&InstallOptions {
            shells: vec![ShellType::Zsh],
            ..options(directory.path())
        })
        .unwrap();
        assert_eq!(
            fs::metadata(&startup).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let backups = fs::read_dir(directory.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("shellbell-backup")
            })
            .count();
        assert_eq!(backups, 1);
    }

    #[test]
    fn dry_run_changes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let mut options = options(directory.path());
        options.dry_run = true;
        let report = install(&options).unwrap();
        assert!(!report.changes.is_empty());
        assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
    }

    #[test]
    fn wsl_and_service_paths_are_deterministic_without_editing_wsl_conf() {
        assert!(is_wsl("6.6.0-microsoft-standard-WSL2"));
        assert!(!is_wsl("6.8.0-generic"));
        assert_eq!(
            service_kind("microsoft-standard-WSL2", true),
            ServiceKind::Systemd
        );
        assert_eq!(
            service_kind("microsoft-standard-WSL2", false),
            ServiceKind::LazyShellStart
        );
        assert_eq!(
            service_kind("6.8.0-generic", false),
            ServiceKind::LazyShellStart
        );
        let unit = systemd_unit(Path::new("/opt/Shell Bell/shellbell"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(!unit.contains("/etc/wsl.conf"));
    }

    #[test]
    fn hooks_use_native_boundaries_and_never_reference_command_content() {
        let bash = hook_content(ShellType::Bash);
        assert!(bash.contains("PROMPT_COMMAND"));
        assert!(bash.contains("trap -p DEBUG"));
        let zsh = hook_content(ShellType::Zsh);
        assert!(zsh.contains("add-zsh-hook preexec"));
        assert!(zsh.contains("add-zsh-hook precmd"));
        let fish = hook_content(ShellType::Fish);
        assert!(fish.contains("--on-event fish_preexec"));
        assert!(fish.contains("--on-event fish_postexec"));
        for content in [bash, zsh, fish] {
            assert!(!content.contains(&["BASH", "COMMAND"].join("_")));
            assert!(!content.contains(&["command", "text"].join("_")));
            assert!(!content.contains("exit status"));
        }
    }
}
