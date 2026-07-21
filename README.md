<div align="center">
  <img src="site/favicon.svg" width="88" height="88" alt="Shellbell logo">

  <h1>Shellbell</h1>

  <p><strong>Private terminal notifications, self-hosted end to end.</strong></p>
  <p>Leave the terminal. Shellbell calls you back when your work is ready for attention.</p>

  <p>
    <a href="https://github.com/KingHacker9000/shellbell/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/KingHacker9000/shellbell?style=for-the-badge&logo=github&label=release"></a>
    <a href="https://github.com/KingHacker9000/shellbell/actions/workflows/ci.yml"><img alt="CI status" src="https://img.shields.io/github/actions/workflow/status/KingHacker9000/shellbell/ci.yml?branch=main&style=for-the-badge&label=CI"></a>
    <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/github/license/KingHacker9000/shellbell?style=for-the-badge"></a>
    <img alt="Linux and WSL" src="https://img.shields.io/badge/Linux%20%7C%20WSL%20%7C%20Pi-supported-2f7b57?style=for-the-badge&logo=linux&logoColor=white">
  </p>

  <p>
    <a href="https://kinghacker9000.github.io/shellbell/"><strong>Guided setup</strong></a>
    ·
    <a href="https://github.com/KingHacker9000/shellbell/releases/latest">Download</a>
    ·
    <a href="docs/">Documentation</a>
    ·
    <a href="SECURITY.md">Security</a>
    ·
    <a href="ROADMAP.md">Roadmap</a>
  </p>
</div>

---

| 🔒 **Private by contract** | 🔔 **Reliable delivery** | 🐚 **Shell-native** |
|---|---|---|
| Observes activity boundaries, never command text, output, environment, directories, exit codes, or inferred outcomes. | Durable local queue, idempotent relay delivery, tagged receivers, and high-urgency Web Push. | Opt-in monitoring for Bash, Zsh, and Fish on Linux x86-64, ARM64, Raspberry Pi, and WSL 2. |

```text
interactive shell → same-user local daemon → private relay → Web Push → your devices
```

Shellbell is an open-source notification system for long-running terminal work. Arm one command, keep monitoring a session, or send a manual ring. When the prompt is ready and remains idle for the configured window, Shellbell notifies the receivers you selected.

> **Current stable release:** `v0.1.1`

## Why Shellbell

Long-running builds, deployments, downloads, model training, and remote jobs often finish while you are away from the terminal. Shellbell lets you stop polling the screen without sending your command history or terminal contents to a hosted service.

- **Own the infrastructure:** run the relay and SQLite database on your server.
- **Own the clients:** pair Linux, WSL 2, and Raspberry Pi machines with revocable source credentials.
- **Own the receivers:** install the PWA on your phone or desktop and tag delivery targets such as `phone`, `pc`, `mobile`, or `desktop`.
- **Keep the privacy boundary:** the shell hook reports timing and state transitions, not what you typed or what the command produced.

## Quick start

The fastest path is the mobile-friendly guided setup:

<p align="center">
  <a href="https://kinghacker9000.github.io/shellbell/"><strong>Open the Shellbell setup site →</strong></a>
</p>

### 1. Self-host the relay

Use the inline **Own the relay** wizard on the setup site, or follow [Deploying Shellbell on a Linux VPS](docs/DEPLOY_VPS.md). The recommended deployment keeps the relay bound to loopback and exposes it only through HTTPS.

### 2. Install the verified CLI

For Bash:

```sh
curl -fsSL https://raw.githubusercontent.com/KingHacker9000/shellbell/main/install.sh \
  | sh -s -- --shell bash
```

Replace `bash` with `zsh` or `fish` as needed. The installer detects Linux x86-64 or ARM64, downloads the latest stable archive, verifies it against the release `SHA256SUMS`, installs to `~/.local/bin`, and validates the binary before replacing an older installation.

Build from source instead:

```sh
cargo install --path crates/shellbell-cli --locked
shellbell install
```

### 3. Pair this machine

```sh
shellbell pair https://shellbell.example.com
```

Approve the pairing request in the Shellbell web app.

### 4. Register a notification receiver

Open the Shellbell web app, go to **Receivers**, allow notifications, and register the browser with tags such as `phone` or `pc`.

### 5. Validate the setup

```sh
shellbell doctor
shellbell ring "Shellbell setup complete" --to all
```

Start a new shell after installing shell integration.

## Everyday use

Notify once after a qualifying command finishes and the prompt stays idle:

```sh
shellbell once --after 5m --idle 30s --to phone
```

Keep monitoring future qualifying command bursts:

```sh
shellbell on --after 2m --idle 45s --to all
```

Send a notification directly:

```sh
shellbell ring "Deployment finished" --to phone
```

Inspect or stop the current shell session:

```sh
shellbell status
shellbell off
```

Name a session so notifications are easier to identify:

```sh
shellbell name "Training run"
```

## How automatic monitoring works

The state model is:

```text
DISARMED → ARMED → ACTIVE → SETTLING
```

A notification is created only after:

1. the shell session is armed;
2. foreground commands accumulate the configured active time;
3. the prompt returns; and
4. no new command begins during the idle window.

Background jobs are not tracked after the prompt returns. Interactive applications that keep foreground control have not completed from the shell's point of view; they can call `shellbell ring` directly when appropriate.

Each terminal pane or nested interactive shell has its own session and must be armed separately.

## Supported environments

| Environment | Status |
|---|---|
| Linux x86-64 | Supported |
| Linux ARM64 | Supported |
| Raspberry Pi OS 64-bit | Supported |
| WSL 2 | Supported |
| Bash | Supported |
| Zsh | Supported |
| Fish | Supported |
| Native PowerShell / CMD | Not yet supported |
| macOS | Not yet supported |

## Operations

Create a consistent relay backup:

```sh
bash scripts/relay-backup.sh
```

Run safe source diagnostics and optionally send a test notification:

```sh
bash scripts/diagnose.sh --send --to all
```

See [Relay backup and restore](docs/BACKUP_RESTORE.md) and [Safe diagnostics](docs/DIAGNOSTICS.md).

## Privacy and security

Shell hooks send small, timeout-bounded messages over a same-user Unix socket. Network delivery happens only in the local daemon. The relay receives an event ID, an optional notification message, and receiver tags.

The product contract explicitly excludes command text, command output, exit codes, environment variables, working directories, process lists, and inferred command success or failure.

See:

- [Security policy](SECURITY.md)
- [Security and privacy model](docs/SECURITY.md)
- [Architecture](docs/ARCHITECTURE.md)
- [HTTP protocol](docs/PROTOCOL.md)
- [Local IPC](docs/LOCAL_IPC.md)

## Documentation

- [Guided setup and self-hosting](https://kinghacker9000.github.io/shellbell/)
- [Linux and WSL installation](docs/LINUX_WSL.md)
- [Raspberry Pi 5 setup](docs/PI5.md)
- [Shell integrations](docs/SHELL_INTEGRATIONS.md)
- [Configuration](docs/CONFIGURATION.md)
- [Deploying on a Linux VPS](docs/DEPLOY_VPS.md)
- [Relay backup and restore](docs/BACKUP_RESTORE.md)
- [Safe diagnostics](docs/DIAGNOSTICS.md)
- [Operations and troubleshooting](docs/OPERATIONS.md)
- [Release process](docs/RELEASING.md)
- [Local development](docs/LOCAL_DEVELOPMENT.md)
- [Roadmap](ROADMAP.md)
- [Changelog](CHANGELOG.md)
- [AI-readable project index](https://kinghacker9000.github.io/shellbell/llms.txt)

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Privacy boundaries are part of the product contract; changes that collect command contents, output, environment, or inferred outcomes require an explicit redesign and security review.

## License

Shellbell is available under the [MIT License](LICENSE).
