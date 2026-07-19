#![cfg(target_os = "linux")]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_shellbell")
}

fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {name} >/dev/null 2>&1")])
        .status()
        .is_ok_and(|status| status.success())
}

struct Fixture {
    _directory: tempfile::TempDir,
    home: PathBuf,
    bin: PathBuf,
    events: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        let bin = directory.path().join("bin");
        let events = directory.path().join("events.log");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&bin).unwrap();
        let fake = bin.join("shellbell");
        fs::write(
            &fake,
            r#"#!/bin/sh
if [ "$1" = "__session-id" ]; then
  printf '%s\n' '00000000-0000-4000-8000-000000000001'
  exit 0
fi
printf '%s|%s\n' "$SHELLBELL_SESSION_ID" "$*" >> "$SHELLBELL_EVENT_LOG"
exit 0
"#,
        )
        .unwrap();
        fs::set_permissions(fake, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            _directory: directory,
            home,
            bin,
            events,
        }
    }

    fn path(&self) -> String {
        format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    fn install(&self, shell: &str) {
        let output = Command::new(binary())
            .args([
                "install",
                "--shell",
                shell,
                "--home",
                self.home.to_str().unwrap(),
            ])
            .env("PATH", self.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "install failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_shell(&self, command: &str, extra: &[(&str, &Path)]) {
        let mut child = Command::new("timeout");
        child
            .args(["-k", "1s", "5s", "sh", "-c", command])
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("PATH", self.path())
            .env("SHELLBELL_EVENT_LOG", &self.events)
            .env(
                "SHELLBELL_TEST_TOKEN",
                format!("{}-{}", "private", uuid::Uuid::new_v4()),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (name, value) in extra {
            child.env(name, value);
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "interactive shell harness timed out or failed; events: {}",
            fs::read_to_string(&self.events).unwrap_or_default()
        );
        let events = fs::read_to_string(&self.events).unwrap_or_default();
        assert!(!events.contains("private-"));
        assert!(!events.contains("SHELLBELL_TEST_TOKEN"));
        assert!(events.contains("__hook session-open"), "{events}");
        assert!(events.contains("__hook command-start"), "{events}");
        assert!(events.contains("__hook prompt-ready"), "{events}");
        assert!(events.contains("__hook session-close"), "{events}");
    }
}

#[test]
fn bash_preserves_prompt_debug_and_nested_session_boundaries() {
    assert!(command_exists("bash"));
    let fixture = Fixture::new();
    let prompt_probe = fixture.home.join("prompt.probe");
    let debug_probe = fixture.home.join("debug.probe");
    fs::write(
        fixture.home.join(".bashrc"),
        "PROMPT_COMMAND='printf p >> \"$PROMPT_PROBE\"'\ntrap 'printf d >> \"$DEBUG_PROBE\"' DEBUG\nPS1='fixture> '\n",
    )
    .unwrap();
    fixture.install("bash");
    fixture.run_shell(
        &format!(
            "bash --noprofile --rcfile {} -i -c 'eval \"$PROMPT_COMMAND\"; printf %s \"$SHELLBELL_TEST_TOKEN\" >/dev/null; exit'",
            fixture.home.join(".bashrc").display()
        ),
        &[("PROMPT_PROBE", &prompt_probe), ("DEBUG_PROBE", &debug_probe)],
    );
    fixture.run_shell(
        &format!(
            "env SHELLBELL_SHELL_PID=1 SHELLBELL_SESSION_ID=00000000-0000-4000-8000-000000000099 bash --noprofile --rcfile {} -i -c 'eval \"$PROMPT_COMMAND\"; true; exit'",
            fixture.home.join(".bashrc").display()
        ),
        &[("PROMPT_PROBE", &prompt_probe), ("DEBUG_PROBE", &debug_probe)],
    );
    assert!(fs::metadata(prompt_probe).unwrap().len() > 0);
    assert!(fs::metadata(debug_probe).unwrap().len() > 0);
    let events = fs::read_to_string(&fixture.events).unwrap();
    let sessions: std::collections::BTreeSet<_> = events
        .lines()
        .filter_map(|line| line.split_once('|').map(|(id, _)| id))
        .collect();
    assert!(
        sessions.len() >= 2,
        "nested Bash must get a distinct session ID: {events}"
    );

    let output = Command::new(binary())
        .args(["uninstall", "--home", fixture.home.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let startup = fs::read_to_string(fixture.home.join(".bashrc")).unwrap();
    assert!(startup.contains("PROMPT_COMMAND="));
    assert!(startup.contains("trap 'printf d"));
    assert!(!startup.contains("shellbell managed integration"));
}

#[test]
fn zsh_preserves_native_oh_my_zsh_and_starship_style_hooks() {
    if !command_exists("zsh") {
        eprintln!("zsh unavailable; CI installs it before this suite");
        return;
    }
    let fixture = Fixture::new();
    let hook_probe = fixture.home.join("zsh-hook.probe");
    fs::write(
        fixture.home.join(".zshrc"),
        "autoload -Uz add-zsh-hook\nfixture_preexec() { print -r -- p >> \"$HOOK_PROBE\" }\nfixture_precmd() { print -r -- c >> \"$HOOK_PROBE\" }\nadd-zsh-hook preexec fixture_preexec\nadd-zsh-hook precmd fixture_precmd\nPROMPT='fixture> '\n",
    )
    .unwrap();
    fixture.install("zsh");
    fixture.run_shell(
        "env ZDOTDIR=\"$HOME\" zsh -i -c 'for fn in ${precmd_functions[@]}; do $fn; done; for fn in ${preexec_functions[@]}; do $fn; done; print -n -- \"$SHELLBELL_TEST_TOKEN\" >/dev/null; __shellbell_zsh_exit; exit'",
        &[("HOOK_PROBE", &hook_probe)],
    );
    fixture.run_shell(
        "env SHELLBELL_SHELL_PID=1 SHELLBELL_SESSION_ID=00000000-0000-4000-8000-000000000099 ZDOTDIR=\"$HOME\" zsh -i -c 'for fn in ${precmd_functions[@]}; do $fn; done; for fn in ${preexec_functions[@]}; do $fn; done; __shellbell_zsh_exit; exit'",
        &[("HOOK_PROBE", &hook_probe)],
    );
    assert!(fs::metadata(hook_probe).unwrap().len() > 0);
    let events = fs::read_to_string(&fixture.events).unwrap();
    let sessions: std::collections::BTreeSet<_> = events
        .lines()
        .filter_map(|line| line.split_once('|').map(|(id, _)| id))
        .collect();
    assert!(
        sessions.len() >= 2,
        "nested Zsh must get a distinct session ID: {events}"
    );
}

#[test]
fn fish_preserves_fish_prompt_native_events_and_nested_sessions() {
    if !command_exists("fish") {
        eprintln!("fish unavailable; CI installs it before this suite");
        return;
    }
    let fixture = Fixture::new();
    let config_dir = fixture.home.join(".config/fish");
    fs::create_dir_all(&config_dir).unwrap();
    let hook_probe = fixture.home.join("fish-hook.probe");
    fs::write(
        config_dir.join("config.fish"),
        "function fish_prompt\n  printf 'fixture> '\nend\nfunction fixture_preexec --on-event fish_preexec\n  echo p >> $HOOK_PROBE\nend\n",
    )
    .unwrap();
    fixture.install("fish");
    fixture.run_shell(
        "fish --interactive -c 'emit fish_postexec; emit fish_preexec; printf %s \"$SHELLBELL_TEST_TOKEN\" >/dev/null; emit fish_exit; exit'",
        &[("HOOK_PROBE", &hook_probe)],
    );
    fixture.run_shell(
        "env SHELLBELL_SHELL_PID=1 SHELLBELL_SESSION_ID=00000000-0000-4000-8000-000000000099 fish --interactive -c 'emit fish_postexec; emit fish_preexec; emit fish_exit; exit'",
        &[("HOOK_PROBE", &hook_probe)],
    );
    assert!(fs::metadata(hook_probe).unwrap().len() > 0);
    let startup = fs::read_to_string(config_dir.join("config.fish")).unwrap();
    assert!(startup.contains("function fish_prompt"));
    let events = fs::read_to_string(&fixture.events).unwrap();
    let sessions: std::collections::BTreeSet<_> = events
        .lines()
        .filter_map(|line| line.split_once('|').map(|(id, _)| id))
        .collect();
    assert!(
        sessions.len() >= 2,
        "nested Fish must get a distinct session ID: {events}"
    );
}
