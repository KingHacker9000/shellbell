# Shellbell

Shellbell is a private, single-owner terminal attention notifier. Milestone 2 adds opt-in activity-burst monitoring for interactive Bash, Zsh, and Fish sessions on Linux x86-64, Linux ARM64, and WSL 2 while preserving Milestone 1 pairing, manual rings, relay, PWA, Web Push, and Docker deployment.

Shellbell watches shell execution boundaries, not commands or jobs. It never collects command text, output, exit codes, environment, process details, history, working directories, or inferred success/failure. A shell rings only after it is armed, foreground commands have accumulated the configured active time, the prompt has returned, and no new command begins during the idle window.

## Quick start

Build and pair the source once:

```sh
cargo build --release -p shellbell-cli
target/release/shellbell pair https://shellbell.example
```

Install all detected supported shells and start the per-user daemon:

```sh
shellbell install
# Or choose explicitly:
shellbell install --shell bash
shellbell install --shell zsh --shell fish
shellbell install --all-shells
```

Start a new shell, then arm that shell session:

```sh
shellbell on
shellbell on --after 5m --idle 1m
shellbell on --to phone

shellbell once
shellbell once --after 30s --idle 10s
shellbell once --to all

shellbell status
shellbell name "FlexAvatar pilot"
shellbell ring "Deployment complete"
shellbell off
```

`on` remains armed across qualifying bursts. `once` disarms after one associated automatic or manual ring. Command-line thresholds and targets affect only the current session and do not rewrite global configuration.

## Activity semantics

The state model is `DISARMED → ARMED → ACTIVE → SETTLING`, with notification recorded on the current burst. Foreground-command wall time is accumulated using a monotonic clock. Prompt idle and time spent typing are not activity.

For example, a 70-second command followed by a 20-second prompt gap and a 60-second command is one 130-second burst when the idle window is 45 seconds. A new command during `SETTLING` cancels that timer and continues the same burst. A short burst resets silently at the deadline. A manual ring associated with a burst suppresses its later automatic duplicate.

Important boundaries:

- `command &` returns the prompt immediately; background work is not tracked.
- An interactive program that retains foreground control has not returned to the prompt, so no automatic ring occurs. It may call `shellbell ring "Task complete"` itself.
- Idle means “the prompt returned and no new command began.” Shellbell does not intercept keystrokes, so a ring can occur while a user is slowly typing but has not pressed Enter.
- Each tmux pane or nested interactive shell gets its own random `SHELLBELL_SESSION_ID` and must be armed independently.

## Local architecture

```text
managed shell hook ── bounded JSON/Unix socket ── per-user shellbell daemon
                                                        │
                                  SQLite sessions + idempotent retry queue
                                                        │ HTTPS
                                                        ▼
Axum relay ── SQLite/idempotency ── tagged Web Push receivers ── PWA
```

Repository layout:

```text
crates/shellbell-cli       public CLI and hidden hook/daemon entry points
crates/shellbell-local     state machine, TOML, IPC, daemon, queue, installer
crates/shellbell-core      secure domain primitives
crates/shellbell-protocol  backwards-compatible relay API types
crates/shellbell-relay     Axum, auth, SQLite, routing, Push, PWA serving
pwa                        React/TypeScript/Vite PWA and service worker
deploy                     Docker Compose, image, Caddy/env examples
docs                       architecture, protocols, security, Linux/WSL guides
```

Hooks perform only a tiny, timeout-bounded local IPC call and fail open. They never perform network traffic. A systemd user service is enabled when available. Linux/WSL environments without a usable user manager use lazy daemon activation on shell start; Shellbell never edits `/etc/wsl.conf`.

## Configuration

`~/.config/shellbell/config.toml` defaults to:

```toml
[activity]
minimum_active = "2m"
idle_for = "45s"

[delivery]
targets = ["phone"]

[display]
automatic_message = "Shell is ready"

[daemon]
queue_limit = 100
event_max_age = "24h"
```

Paired source credentials remain separately protected in `~/.config/shellbell/config.json`. `shellbell config` prints paths and safe effective settings but never the source token.

## Safe install inspection and removal

Tests and audits can target an alternate home without touching the current user:

```sh
shellbell install --all-shells --dry-run --home /tmp/test-home
shellbell install --all-shells --home /tmp/test-home
shellbell uninstall --home /tmp/test-home
```

Installation backs up existing startup files, appends one marked source line, and preserves permissions and line endings. It is idempotent. Uninstall removes only Shellbell-managed lines/files and preserves configuration, state, and credentials unless `--purge` is explicitly supplied.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets --locked

npm --prefix pwa ci
npm --prefix pwa run typecheck
npm --prefix pwa run lint
npm --prefix pwa test
npm --prefix pwa run build
npm --prefix pwa run validate:manifest

SHELLBELL_OWNER_BOOTSTRAP_TOKEN=placeholder-owner-token-at-least-32-bytes \
SHELLBELL_VAPID_PUBLIC_KEY=placeholder-public \
SHELLBELL_VAPID_PRIVATE_KEY=placeholder-private \
docker compose -f deploy/docker-compose.yml config --quiet
docker build -f deploy/Dockerfile -t shellbell:milestone-2 .
```

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [HTTP protocol](docs/PROTOCOL.md)
- [Local IPC](docs/LOCAL_IPC.md)
- [Linux and WSL installation](docs/LINUX_WSL.md)
- [Shell integrations](docs/SHELL_INTEGRATIONS.md)
- [Threat model and privacy](docs/SECURITY.md)
- [Local development and manual verification](docs/LOCAL_DEVELOPMENT.md)
- [Milestones](docs/MILESTONES.md)

## Known limitations

Milestone 2 does not support PowerShell, Windows CMD, macOS, job/process tracking, output-silence detection, prompt-text scraping, or application-specific integrations. Automatic active duration remains local and is not added to the relay payload. A daemon outage during an ACTIVE command discards that incomplete burst on restart rather than guessing when foreground control returned. Real Web Push still requires HTTPS and a browser/provider network path.
