# Deploying Shellbell on a Linux VPS

This guide is provider-neutral. It applies to a small Linux VPS or self-hosted
Docker machine with a public domain and ports 80 and 443 available.

Example public URL:

```text
https://shellbell.example.com
```

Replace the example domain with a domain you control.

## Recommended layout

```text
/opt/stacks/shellbell/
├── compose.yaml
├── .env
├── data/
└── README.md
```

Recommended properties:

- The relay binds only to `127.0.0.1:8080`.
- Caddy, nginx, or another reviewed reverse proxy is the only public ingress.
- Only ports 80 and 443 are exposed publicly.
- SQLite data persists outside the container.
- The secret environment file has mode `0600`.
- Container memory, process, and log growth are bounded.
- Images use immutable release or source-commit tags.
- Deployment files and persistent data are backed up before replacement.

## Build or obtain the image

Build the image from a reviewed source revision:

```sh
docker build \
  -f deploy/Dockerfile \
  -t shellbell:<release-or-commit> \
  .
```

Record the exact image tag and source revision in the host's private runbook.

## Create the stack

Copy `deploy/docker-compose.yml` to the stack directory as `compose.yaml`.
Keep the repository example free of production secrets.

Create `/opt/stacks/shellbell/.env` with:

```text
SHELLBELL_OWNER_BOOTSTRAP_TOKEN=<random secret>
SHELLBELL_VAPID_PUBLIC_KEY=<public VAPID key>
SHELLBELL_VAPID_PRIVATE_KEY=<private VAPID key>
SHELLBELL_VAPID_SUBJECT=mailto:admin@example.com
SHELLBELL_HISTORY_RETENTION_DAYS=14
SHELLBELL_INSECURE_LOCAL_HTTP=false
RUST_LOG=shellbell_relay=info,tower_http=info
```

Then restrict it:

```sh
chmod 600 /opt/stacks/shellbell/.env
```

Never commit or print the bootstrap token, VAPID private key, source tokens,
Push subscription endpoints, or encryption keys.

## Validate before starting

```sh
cd /opt/stacks/shellbell
docker compose config --quiet
docker compose up -d
docker compose ps
curl -fsS http://127.0.0.1:8080/health
```

Port 8080 must remain loopback-only.

## Configure HTTPS

Use `deploy/Caddyfile.example` as a starting point and replace the example
domain. Validate the complete proxy configuration before reloading it.

For Caddy:

```sh
caddy validate --config /etc/caddy/Caddyfile
systemctl reload caddy
curl -fsS https://shellbell.example.com/health
```

The public health request should return a JSON response with status `ok`.

## Safe upgrades

Before replacing files or data:

```sh
cd /opt/stacks/shellbell
cp -a compose.yaml "compose.yaml.backup.$(date +%F-%H%M%S)"
cp -a data "data.backup.$(date +%F-%H%M%S)"
```

Validate the new Compose and reverse-proxy configurations before activation.
Retain a known-good image tag and a tested SQLite backup for rollback.

## Host-private information

Keep real domains, server addresses, provider account details, firewall rules,
backup destinations, and operational contacts in a private host runbook rather
than this public repository.
