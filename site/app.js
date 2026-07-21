const platform = document.querySelector('#platform');
const shell = document.querySelector('#shell');
const relayUrl = document.querySelector('#relay-url');
const selfDomain = document.querySelector('#self-domain');
const ownerEmail = document.querySelector('#owner-email');

const command = (id, value) => {
  document.querySelector(`#${id}`).textContent = value;
};

const normalizeRelayUrl = (value) => {
  const candidate = value.trim() || 'https://shellbell.example.com';
  const withScheme = /^https?:\/\//i.test(candidate) ? candidate : `https://${candidate}`;

  try {
    const parsed = new URL(withScheme);
    return ['http:', 'https:'].includes(parsed.protocol)
      ? parsed.href.replace(/\/$/, '')
      : 'https://shellbell.example.com';
  } catch {
    return 'https://shellbell.example.com';
  }
};

const validDomain = (value) => {
  const candidate = value.trim().toLowerCase();
  const hostname = /^(?=.{1,253}$)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$/;
  return hostname.test(candidate) ? candidate : 'shellbell.example.com';
};

const validEmail = (value) => {
  const candidate = value.trim();
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(candidate)
    ? candidate
    : 'owner@example.com';
};

function renderClientCommands() {
  const selectedShell = shell.value;
  const url = normalizeRelayUrl(relayUrl.value);
  const platformNote = platform.value === 'pi'
    ? 'uname -m  # expected: aarch64\n'
    : platform.value === 'wsl'
      ? 'uname -a  # confirm WSL 2\n'
      : '';

  command(
    'install-command',
    `${platformNote}curl -fsSL https://raw.githubusercontent.com/KingHacker9000/shellbell/main/install.sh | sh -s -- --shell ${selectedShell}`,
  );
  command('pair-command', `shellbell pair ${url}`);
  command('open-command', `Open ${url} and register this browser as a receiver.`);
  command(
    'test-command',
    'shellbell doctor\nshellbell ring "Shellbell setup complete" --to all',
  );
}

function renderHostCommands() {
  const domain = validDomain(selfDomain.value);
  const email = validEmail(ownerEmail.value);

  command(
    'host-prepare-command',
    `sudo install -d -m 0755 /opt/stacks/shellbell
sudo chown "$USER":"$USER" /opt/stacks/shellbell
cd /opt/stacks/shellbell
curl -fsSLo compose.yaml https://raw.githubusercontent.com/KingHacker9000/shellbell/main/deploy/docker-compose.yml`,
  );

  command(
    'host-secrets-command',
    `cd /opt/stacks/shellbell
umask 077
OWNER_TOKEN="$(openssl rand -base64 48 | tr -d '\\n')"
VAPID_JSON="$(npx --yes web-push generate-vapid-keys --json)"
VAPID_PUBLIC_KEY="$(printf '%s' "$VAPID_JSON" | jq -r '.publicKey')"
VAPID_PRIVATE_KEY="$(printf '%s' "$VAPID_JSON" | jq -r '.privateKey')"
cat > .env <<EOF
SHELLBELL_OWNER_BOOTSTRAP_TOKEN=$OWNER_TOKEN
SHELLBELL_VAPID_PUBLIC_KEY=$VAPID_PUBLIC_KEY
SHELLBELL_VAPID_PRIVATE_KEY=$VAPID_PRIVATE_KEY
SHELLBELL_VAPID_SUBJECT=mailto:${email}
SHELLBELL_HISTORY_RETENTION_DAYS=14
SHELLBELL_PORT=8080
SHELLBELL_INSECURE_LOCAL_HTTP=false
RUST_LOG=shellbell_relay=info,tower_http=info
EOF
chmod 600 .env
unset OWNER_TOKEN VAPID_JSON VAPID_PUBLIC_KEY VAPID_PRIVATE_KEY`,
  );

  command(
    'host-start-command',
    `cd /opt/stacks/shellbell
docker compose config --quiet
docker compose pull
docker compose up -d
docker compose ps
curl -fsS http://127.0.0.1:8080/health`,
  );

  command(
    'host-caddy-command',
    `sudo install -d -m 0755 /etc/caddy/sites
sudo tee /etc/caddy/sites/shellbell.caddy >/dev/null <<'CADDY'
${domain} {
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
sudo grep -Fqx 'import /etc/caddy/sites/*.caddy' /etc/caddy/Caddyfile || \\
  printf '\\nimport /etc/caddy/sites/*.caddy\\n' | sudo tee -a /etc/caddy/Caddyfile >/dev/null
sudo caddy validate --config /etc/caddy/Caddyfile
sudo systemctl reload caddy
curl -fsS https://${domain}/health`,
  );

  command(
    'host-finish-command',
    `printf 'Open https://${domain} in your browser.\\n'
printf 'Bootstrap token (copy locally and never share it):\\n'
sudo awk -F= '$1 == "SHELLBELL_OWNER_BOOTSTRAP_TOKEN" { print substr($0, index($0, "=") + 1) }' /opt/stacks/shellbell/.env`,
  );
}

function renderCommands() {
  renderClientCommands();
  renderHostCommands();
}

function legacyCopy(text) {
  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.setAttribute('readonly', '');
  textarea.style.position = 'fixed';
  textarea.style.opacity = '0';
  document.body.appendChild(textarea);
  textarea.select();
  const copied = document.execCommand('copy');
  textarea.remove();
  return copied;
}

async function copyText(text) {
  if (navigator.clipboard && window.isSecureContext) {
    await navigator.clipboard.writeText(text);
    return true;
  }

  return legacyCopy(text);
}

for (const input of [platform, shell, relayUrl, selfDomain, ownerEmail]) {
  input.addEventListener('input', renderCommands);
}

for (const button of document.querySelectorAll('[data-copy]')) {
  button.addEventListener('click', async () => {
    const target = document.querySelector(`#${button.dataset.copy}`);
    const original = button.textContent;

    try {
      const copied = await copyText(target.textContent);
      button.textContent = copied ? 'Copied' : 'Select text';
    } catch {
      button.textContent = 'Select text';
    }

    setTimeout(() => { button.textContent = original; }, 1400);
  });
}

renderCommands();
