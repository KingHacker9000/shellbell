# Shellbell

Shellbell sends a notification when your terminal is ready for attention.

It is a private, self-hosted tool for people who leave long-running commands in a terminal and do not want to keep checking the screen. Shellbell supports manual notifications and opt-in automatic monitoring for interactive Bash, Zsh, and Fish sessions.

Shellbell watches shell activity boundaries, not command contents. It does **not** collect command text, output, exit codes, environment variables, working directories, process lists, or inferred success and failure.

> The current release line is `0.1.0`. Prereleases use tags such as `v0.1.0-rc.1`.

## What it does

- Sends notifications to tagged browser receivers such as `phone` and `pc`.
- Provides `on`, `once`, `off`, `status`, `name`, and manual `ring` commands.
- Accumulates foreground command time across short prompt gaps.
- Delivers through a private relay, installable PWA, and Web Push.
- Keeps a durable local retry queue when the relay is temporarily unavailable.
- Runs on Linux x86-64, Linux ARM64, Raspberry Pi OS, and WSL 2.

## Quick start from source

### 1. Self-host the relay

Deploy the relay and PWA on a Linux host with HTTPS. See [Deploying on a Linux VPS](docs/DEPLOY_VPS.md).

### 2. Build and install the CLI

```sh
cargo install --path crates/shellbell-cli --locked
```

Prebuilt Linux x86-64 and ARM64 CLI archives are also attached to tagged GitHub releases. Verify downloads with the published `SHA256SUMS` file before installing the binary.

### 3. Pair this machine

```sh
shellbell pair https://shellbell.example.com
```

Approve the pairing request in the Shellbell web app.

### 4. Register a notification receiver

Open the Shellbell web app, go to **Receivers**, allow notifications, and register the browser with tags such as `phone` or `pc`.

### 5. Install shell integration

```sh
shellbell install
```

Start a new shell after installation.

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

## Privacy and security

Shell hooks send small, timeout-bounded messages over a same-user Unix socket. Network delivery happens only in the local daemon. The relay receives an event ID, an optional notification message, and receiver tags.

See:

- [Security policy](SECURITY.md)
- [Security and privacy model](docs/SECURITY.md)
- [Architecture](docs/ARCHITECTURE.md)
- [HTTP protocol](docs/PROTOCOL.md)
- [Local IPC](docs/LOCAL_IPC.md)

## Documentation

- [Linux and WSL installation](docs/LINUX_WSL.md)
- [Shell integrations](docs/SHELL_INTEGRATIONS.md)
- [Configuration](docs/CONFIGURATION.md)
- [Deploying on a Linux VPS](docs/DEPLOY_VPS.md)
- [Operations and troubleshooting](docs/OPERATIONS.md)
- [Release process](docs/RELEASING.md)
- [Local development](docs/LOCAL_DEVELOPMENT.md)
- [Roadmap](ROADMAP.md)
- [Changelog](CHANGELOG.md)

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Privacy boundaries are part of the product contract and changes that collect command contents, output, environment, or inferred outcomes will not be accepted without an explicit redesign and security review.

## License

Shellbell is available under the [MIT License](LICENSE).
