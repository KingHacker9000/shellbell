# Linux and WSL installation

Shellbell supports Linux x86-64, Linux ARM64 (including Raspberry Pi), and WSL 2 distributions with Bash, Zsh, and Fish. It does not currently support native Windows PowerShell/CMD or macOS.

## Install

Place the built `shellbell` binary somewhere on `PATH`, preserve the existing pairing credential, then run:

```sh
shellbell install
```

With no shell option, installed supported executables are detected. Explicit forms are:

```sh
shellbell install --shell bash
shellbell install --shell zsh
shellbell install --shell fish
shellbell install --shell bash --shell zsh
shellbell install --all-shells
```

Inspect safely first or target a test home:

```sh
shellbell install --all-shells --dry-run
mkdir -p /tmp/shellbell-test-home
shellbell install --all-shells --home /tmp/shellbell-test-home
shellbell uninstall --home /tmp/shellbell-test-home
```

Installation creates the default TOML only when absent, writes managed hooks under `~/.config/shellbell/hooks`, backs up every existing startup file before its first modification, and appends one marked source line. It preserves existing bytes, permissions, and CRLF/LF style and does not arm a shell. Re-running it does not duplicate entries or backups.

## systemd user service

When `systemctl --user show-environment` succeeds within a bounded check, installation writes `~/.config/systemd/user/shellbell.service` and runs:

```sh
systemctl --user daemon-reload
systemctl --user enable --now shellbell.service
systemctl --user status shellbell.service
```

The service uses `Restart=on-failure`, a five-second restart delay, five starts per 60 seconds, and `NoNewPrivileges=true`. It runs as the current user and requires no root.

## WSL 2

Shellbell detects WSL from `/proc/sys/kernel/osrelease`. If the distribution already has systemd enabled and its user manager works, it uses the same user service. Shellbell never reads or modifies startup command content and never modifies `/etc/wsl.conf`.

For a WSL distribution without systemd, or a normal Linux environment without a usable user manager, install reports `shell-start lazy daemon activation`. The managed hook starts `shellbell __daemon` detached on shell open if the private socket is unavailable. All later hooks make only local IPC calls. This fallback requires no root and no separate startup entry beyond the managed integration source line.

## Uninstall

```sh
shellbell uninstall
```

This disarms the current integrated session when reachable, disables/removes only the marked systemd unit, asks the same-user daemon to stop, removes marked startup lines and managed hook files, and creates backups before startup removal. Configuration, local queue/state, and pairing credentials are preserved.

Explicitly remove them only when intended:

```sh
shellbell uninstall --purge
```

Uninstall is idempotent and never removes unrelated startup content. An alternate `--home` never manages the real service.

## Troubleshooting

```sh
shellbell doctor
shellbell config
shellbell status
systemctl --user status shellbell.service
```

`doctor` labels each check `OK`, `WARNING`, or `FAILURE`: TOML, pairing, credential permissions, relay health/token, daemon/socket round trip, systemd/fallback, hook readability/duplicates, durable queue, and current session. It never prints the source token.

If the current process says it is not integrated, start a new interactive shell after installation. If WSL systemd is desired, enable it manually according to Microsoft/distribution guidance and restart WSL; Shellbell intentionally does not edit system configuration.
