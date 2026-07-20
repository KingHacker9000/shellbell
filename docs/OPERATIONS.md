# Operations and troubleshooting

This guide verifies a private Shellbell deployment and helps isolate delivery problems.

## Reference deployment

A typical installation uses:

```text
/opt/stacks/shellbell/
├── compose.yaml
├── .env
└── data/
```

Recommended properties:

- the relay binds only to `127.0.0.1:8080`;
- an HTTPS reverse proxy is the only public ingress;
- the `.env` file is mode `0600`;
- SQLite data persists outside the container;
- images use immutable release or commit tags; and
- container memory and log growth are bounded.

## Basic health checks

```sh
cd /opt/stacks/shellbell
docker compose config --quiet
docker compose ps
curl -fsS http://127.0.0.1:8080/health
```

Also verify the public HTTPS endpoint:

```sh
curl -fsS https://shellbell.example.com/health
```

## End-to-end verification

1. Sign in to the PWA.
2. Register an enabled browser receiver with a tag such as `phone` or `pc`.
3. Pair a source machine.
4. Send a manual notification.
5. Install a supported shell hook.
6. Open a new shell.
7. Run an automatic notification test.
8. Confirm `once` returns to `off` and `disarmed` after delivery.

Manual test:

```sh
shellbell ring "Shellbell manual test" --to phone
```

Automatic test:

```sh
shellbell off
shellbell name "Shellbell test"
shellbell once --after 5s --idle 5s --to phone
sleep 7
```

After the prompt returns, do not submit another command until the idle window expires.

## Understanding `Ring queued`

`Ring queued` means the local daemon accepted the event. Relay submission happens asynchronously.

During a temporary outage, the durable queue keeps the original event ID and retries with bounded backoff. Use:

```sh
shellbell doctor
shellbell status
```

## Notification troubleshooting

Check each layer separately:

1. The local daemon accepted or queued the event.
2. The source is still authorized.
3. The relay recorded the ring.
4. At least one enabled receiver matched the requested tags.
5. The Push provider accepted the delivery.
6. The browser still has notification permission.
7. The operating system did not suppress or delay the notification.

A ring visible in the PWA proves that the source-to-relay path worked. A relay delivery marked `delivered` means the Push provider accepted the request; it does not guarantee that the operating system displayed a banner.

When inspecting relay data, print only receiver names, tags, delivery status, diagnostics, and timestamps. Never print Push endpoints or encryption keys.

## WSL daemon startup

A WSL distribution without a usable systemd user manager uses shell-start lazy activation. `shellbell doctor` may warn that systemd is unavailable while the daemon, socket, pairing, and queue remain healthy.

## Safe upgrades

Before replacing an image or deployment file:

```sh
cd /opt/stacks/shellbell
cp -a compose.yaml "compose.yaml.backup.$(date +%F-%H%M%S)"
cp -a data "data.backup.$(date +%F-%H%M%S)"
```

Validate the new Compose configuration before starting it. Keep the previous immutable image tag and a tested SQLite backup until the upgrade is verified.

## Privacy boundary

The relay stores notification messages and privacy-safe routing metadata. It does not receive command text, command output, exit codes, current directories, environment variables, process lists, or inferred success and failure.
