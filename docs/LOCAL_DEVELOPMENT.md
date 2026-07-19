# Local development

Prerequisites: Rust 1.97, Node.js 24 with npm, Docker with Compose v2, and `openssl`.

## Native development

Install and validate the PWA:

```sh
npm --prefix pwa ci
npm --prefix pwa run typecheck
npm --prefix pwa run lint
npm --prefix pwa test
npm --prefix pwa run build
npm --prefix pwa run validate:manifest
```

Run Rust checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

For split native development, provide the relay environment, run `cargo run -p shellbell-relay`, then `npm --prefix pwa run dev`. Vite proxies `/api` and `/health` to port 8080. The relay generates an owner token at `$SHELLBELL_DATA_DIR/owner-bootstrap-token` if the environment token is omitted. A VAPID key pair is still required.

## Docker Compose

Create configuration without printing generated secrets into shell history where practical:

```sh
cp deploy/.env.example deploy/.env
openssl rand -base64 48 | tr -d '\n'
npx --yes web-push generate-vapid-keys --json
```

Paste the owner value and VAPID pair into `deploy/.env`, then:

```sh
docker compose --env-file deploy/.env -f deploy/docker-compose.yml config
docker compose --env-file deploy/.env -f deploy/docker-compose.yml build
docker compose --env-file deploy/.env -f deploy/docker-compose.yml up -d
curl --fail http://127.0.0.1:8080/health
docker compose --env-file deploy/.env -f deploy/docker-compose.yml logs -f shellbell
docker compose --env-file deploy/.env -f deploy/docker-compose.yml down
```

Reset local development data (destructive and not recoverable unless the volume was backed up):

```sh
docker compose --env-file deploy/.env -f deploy/docker-compose.yml down --volumes
```

The Compose port binds only to loopback. For real Push, use HTTPS through Caddy, set `SHELLBELL_INSECURE_LOCAL_HTTP=false`, and keep port 8080 firewalled from the internet.
