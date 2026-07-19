# Shellbell

Shellbell is a private, single-user, self-hosted terminal attention notifier. Milestone 1 implements the complete manual path from a Linux/WSL CLI through pairing and an SQLite relay to an installable PWA and VAPID Web Push.

Shellbell is not a job manager. It does not collect, store, transmit, or infer commands, output, exit codes, processes, environment variables, working directories, shell history, shell identity, or success/failure. A ring contains only a random event ID, source identity/name, optional text deliberately supplied to `shellbell ring`, timestamp, and receiver tags.

## Current architecture

The Rust `shellbell` CLI pairs a source and submits manual rings. An Axum relay authenticates one owner and send-only sources, applies SQLx SQLite migrations, records idempotent rings, routes to tagged receivers through an internal Push interface, and serves the built React/Vite PWA. Browser sessions use secure cookies plus CSRF protection. Production Push is VAPID-backed; automated tests use an in-memory fake.

Repository layout:

```text
crates/shellbell-cli       CLI configuration, pairing, ring, doctor
crates/shellbell-core      secure domain primitives
crates/shellbell-protocol  shared API types and validation
crates/shellbell-relay     API, auth, SQLite, routing, Push, PWA serving
pwa                        React/TypeScript/Vite PWA and service worker
deploy                     Docker Compose, image, Caddy/env examples
docs                       protocol, architecture, security, operations
```

## Prerequisites

- Rust 1.97
- Node.js 24 and npm
- Docker Engine/desktop with Compose v2
- OpenSSL (for local secret generation)

## Validate locally

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features

npm --prefix pwa ci
npm --prefix pwa run typecheck
npm --prefix pwa run lint
npm --prefix pwa test
npm --prefix pwa run build
npm --prefix pwa run validate:manifest
```

## Run with Docker Compose

```sh
cp deploy/.env.example deploy/.env
openssl rand -base64 48 | tr -d '\n'
npx --yes web-push generate-vapid-keys --json
```

Paste those values into `deploy/.env`, then run:

```sh
docker compose --env-file deploy/.env -f deploy/docker-compose.yml config
docker compose --env-file deploy/.env -f deploy/docker-compose.yml build
docker compose --env-file deploy/.env -f deploy/docker-compose.yml up -d
curl --fail http://127.0.0.1:8080/health
```

Local HTTP is suitable for the localhost application UI, but browser Push support and production cookies require a secure context. For a real notification test, put the relay behind the example Caddy HTTPS configuration and set `SHELLBELL_INSECURE_LOCAL_HTTP=false`.

## Manual pairing and ring walkthrough

1. Open the PWA and enter `SHELLBELL_OWNER_BOOTSTRAP_TOKEN` to bootstrap/sign in.
2. Build or run the CLI: `cargo run -p shellbell-cli -- pair http://localhost:8080` (use the HTTPS relay URL for real Push).
3. In **Sources**, match the displayed code and approve it. The CLI stores its send-only token in the per-user config directory with mode `0600` on Unix.
4. In **Receivers**, give the browser a name, select any tags, and press **Allow notifications and register**. No permission prompt occurs before this explicit action.
5. Send `cargo run -p shellbell-cli -- ring "Test message"`. Target examples are `--to phone`, `--to pc`, or `--to all`.
6. Confirm the notification and the entry in **Rings**. Run `cargo run -p shellbell-cli -- doctor` to verify relay and credential health.

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Exact HTTP protocol](docs/PROTOCOL.md)
- [Threat model](docs/SECURITY.md)
- [Local development and reset](docs/LOCAL_DEVELOPMENT.md)
- [Future Lightsail plan](docs/DEPLOY_LIGHTSAIL.md)
- [Milestones](docs/MILESTONES.md)

## Current limitations

Milestone 1 supports manual rings and Linux/WSL CLI execution only. It has no automatic shell hooks or monitoring, background delivery retries, multi-user accounts, command/job management, native mobile app, Windows/macOS packaging, or deployed Lightsail infrastructure. The in-process rate limiter resets when the relay restarts. Real Web Push requires HTTPS and a browser/provider network path.
