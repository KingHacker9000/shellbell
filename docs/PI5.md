# Raspberry Pi 5 setup

Shellbell supports 64-bit Raspberry Pi OS and other 64-bit Linux distributions on Raspberry Pi 5.

## 1. Confirm the operating system

```sh
uname -s
uname -m
getconf GNU_LIBC_VERSION
```

Expected architecture:

```text
aarch64
```

Published ARM64 archives require GLIBC 2.35 or newer. A 32-bit operating system is not supported.

## 2. Install the verified release

For Bash:

```sh
curl -fsSL https://raw.githubusercontent.com/KingHacker9000/shellbell/main/install.sh \
  | sh -s -- --shell bash
```

For Zsh or Fish, replace `bash` with `zsh` or `fish`. The installer:

- resolves the latest stable GitHub release;
- downloads the Linux ARM64 archive and `SHA256SUMS`;
- verifies the archive before extraction;
- executes and validates the downloaded binary before replacing any existing installation;
- installs `shellbell` to `~/.local/bin`;
- installs only the shell integration you request.

Start a new shell after installation.

## 3. Pair the Pi

```sh
shellbell pair https://shellbell.example.com
```

Approve the displayed pairing code in the Shellbell web app.

## 4. Validate the installation

From a repository checkout:

```sh
shellbell doctor
bash scripts/diagnose.sh --send --to all
```

When using only the installed release:

```sh
shellbell doctor
shellbell ring "Raspberry Pi 5 test" --to all
```

Confirm the notification on each intended receiver.

## 5. Test automatic monitoring

Run these as one submission:

```sh
shellbell once --after 5s --idle 5s --to all
sleep 7
```

Wait at least six seconds after the prompt returns, then run:

```sh
shellbell status
```

Expected final state:

```text
Mode: off
State: disarmed
Daemon connection: connected
```

## Notes

- A systemd user manager is preferred but not required. Shellbell falls back to shell-start lazy daemon activation.
- Each terminal pane has its own session and must be armed separately.
- Shellbell does not inspect command text, output, working directories, environment variables, or process lists.
