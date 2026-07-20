# Deploying Shellbell on a Linux VPS

This guide is provider-neutral. It applies to a small Linux VPS or self-hosted
Docker machine with a public domain and ports 80 and 443 available.

The [guided setup site](https://kinghacker9000.github.io/shellbell/#self-host)
generates the same commands from your relay domain and owner email. This page
keeps the full operational detail and review checklist.

Example public URL:

```text
https://shellbell.example.com
```

Replace the example domain with a domain you control.

## Prerequisites

Before deployment:

- Point the domain's A or AAAA record at the host.
- Allow inbound TCP ports 80 and 443.
- Install Docker with the Compose plugin, Caddy, curl, OpenSSL, Node.js/npm,
  and `jq`.
- Do not expose port 8080 publicly.

## Recommended layout

```text
/opt/stacks/shellbell/
├── compose.yaml
├── .env
└── private runbook or README
```

Recommended properties:

- The relay binds only to `127.0.0.1:8080`.
- Caddy, nginx, or another reviewed reverse proxy is the only public ingress.
- Only ports 80 and 443 are exposed publicly.
- SQLite data persists in the `shellbell-data` Docker volume.
- The secret environment file has mode `0600`.
- Container memory, process, and log growth are bounded by the host.
- Images use immutable release tags or digests.
- Deployment files and persistent data are backed up before replacement.

## Create the stack

Create the application directory and download the reviewed Compose definition:

```sh
sudo install -d -m 0755 /opt/stacks/shellbell
sudo chown "$USER":"$USER" /opt/stacks/shellbell
cd /opt/stacks/shellbell
curl -fsSLo compose.yaml \
  https://raw.githubusercontent.com/KingHacker9000/shellbell/main/deploy/docker-compose.yml
```

The public Compose definition uses the released multi-architecture image:

```text
ghcr.io/kinghacker9000/shellbell:v0.1.1
```

For stronger production pinning, replace the release tag with the verified OCI
index digest from the GitHub release acceptance record. Upgrade only after the
new image and rollback path have been tested.

## Generate the private environment

Generate the owner bootstrap token and VAPID key pair on the server:

```sh
cd /opt/stacks/shellbell
umask 077
OWNER_TOKEN="$(openssl rand -base64 48 | tr -d '\n')"
VAPID_JSON="$(npx --yes web-push generate-vapid-keys --json)"
VAPID_PUBLIC_KEY="$(printf '%s' "$VAPID_JSON" | jq -r '.publicKey')"
VAPID_PRIVATE_KEY="$(printf '%s' "$VAPID_JSON" | jq -r '.privateKey')"
cat > .env <<EOF
SHELLBELL_OWNER_BOOTSTRAP_TOKEN=$OWNER_TOKEN
SHELLBELL_VAPID_PUBLIC_KEY=$VAPID_PUBLIC_KEY
SHELLBELL_VAPID_PRIVATE_KEY=$VAPID_PRIVATE_KEY
SHELLBELL_VAPID_SUBJECT=mailto:owner@example.com
SHELLBELL_HISTORY_RETENTION_DAYS=14
SHELLBELL_PORT=8080
SHELLBELL_INSECURE_LOCAL_HTTP=false
RUST_LOG=shellbell_relay=info,tower_http=info
EOF
chmod 600 .env
unset OWNER_TOKEN VAPID_JSON VAPID_PUBLIC_KEY VAPID_PRIVATE_KEY
```

Never commit or publish the bootstrap token, VAPID private key, source tokens,
Push subscription endpoints, or encryption keys.

## Validate and start

```sh
cd /opt/stacks/shellbell
docker compose config --quiet
docker compose pull
docker compose up -d
docker compose ps
curl -fsS http://127.0.0.1:8080/health
```

The health request should return a JSON response with status `ok`. Confirm the
host firewall does not expose port 8080.

## Configure HTTPS with Caddy

Create a dedicated site file:

```sh
sudo install -d -m 0755 /etc/caddy/sites
sudo tee /etc/caddy/sites/shellbell.caddy >/dev/null <<'CADDY'
shellbell.example.com {
    encode zstd gzip
    reverse_proxy 127.0.0.1:8080

    header {
        Strict-Transport-Security "max-age=31536000; includeSubDomains"
        X-Content-Type-Options "nosniff"
        Referrer-Policy "no-referrer"
        Permissions-Policy "camera=(), microphone=(), geolocation=()"
    }
}
CADDY
```

Import the site directory once, validate the complete configuration, and reload:

```sh
sudo grep -Fqx 'import /etc/caddy/sites/*.caddy' /etc/caddy/Caddyfile || \
  printf '\nimport /etc/caddy/sites/*.caddy\n' | \
  sudo tee -a /etc/caddy/Caddyfile >/dev/null
sudo caddy validate --config /etc/caddy/Caddyfile
sudo systemctl reload caddy
curl -fsS https://shellbell.example.com/health
```

## Claim the owner session

Open the HTTPS relay URL in a browser. Retrieve the bootstrap token locally on
the host, paste it into the owner sign-in form, and do not share it:

```sh
sudo awk -F= \
  '$1 == "SHELLBELL_OWNER_BOOTSTRAP_TOKEN" { print substr($0, index($0, "=") + 1) }' \
  /opt/stacks/shellbell/.env
```

After signing in, install the PWA, allow notifications, and register the first
receiver. Owner sessions are revocable and the bootstrap token is not stored in
browser storage.

## Safe upgrades

Before replacing configuration or data, use the project backup helper or take
a tested equivalent snapshot. Keep the old image reference available for
rollback.

```sh
cd /opt/stacks/shellbell
cp -a compose.yaml "compose.yaml.backup.$(date +%F-%H%M%S)"
docker compose config --quiet
docker compose pull
docker compose up -d
curl -fsS http://127.0.0.1:8080/health
curl -fsS https://shellbell.example.com/health
```

## Host-private information

Keep real domains, server addresses, provider account details, firewall rules,
backup destinations, immutable production digests, and operational contacts in
a private host runbook rather than this public repository.
