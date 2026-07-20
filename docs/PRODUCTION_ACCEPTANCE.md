# Private production deployment and acceptance

The private Shellbell deployment is available at:

```text
https://shellbell.ashishajin.com
```

## Production layout

The current AWS Lightsail deployment follows the host runbook conventions:

- Stack directory: `/opt/stacks/shellbell`
- Compose file: `/opt/stacks/shellbell/compose.yaml`
- Secret environment file: `/opt/stacks/shellbell/.env` with mode `0600`
- Persistent SQLite data: `/opt/stacks/shellbell/data`
- Container name: `shellbell`
- Backend binding: `127.0.0.1:8080`
- Public ingress: Caddy on ports 80 and 443
- Public URL: `https://shellbell.ashishajin.com`
- Immutable image tags derived from the source commit
- Container memory limit: 160 MiB
- PID limit: 128
- Bounded Docker logs
- Local and public health checks

Port 8080 must remain private. Caddy is the only public ingress.

Before replacing deployment files or persistent data, create timestamped backups and validate both Docker Compose and the complete Caddy configuration. Never print the owner bootstrap token, VAPID private key, source token, or Push subscription endpoint while diagnosing the deployment.

## Confirmed acceptance path

The following production path was verified manually:

1. Bootstrap the owner session in the PWA.
2. Register a browser receiver and grant site notification permission.
3. Pair a WSL source with `shellbell pair https://shellbell.ashishajin.com`.
4. Send a manual notification with `shellbell ring`.
5. Confirm the local durable queue retries after a temporary relay timeout without changing the event ID.
6. Confirm the relay records the receiver match and Web Push delivery.
7. Install the Bash hook with `shellbell install --shell bash`.
8. Open a new interactive Bash session.
9. Arm once mode and run a qualifying foreground command from the next prompt.
10. Confirm the notification arrives after the command returns and the idle window expires.
11. Confirm once mode automatically returns to `off` and `disarmed`.

## Manual notification test

Register a receiver carrying the `pc` tag, then run:

```sh
shellbell ring "Shellbell manual test" --to pc
```

`Ring queued` means the local daemon accepted the event. The daemon then submits it asynchronously to the relay. When connectivity is unavailable, the event remains in the durable local queue and is retried with the same idempotency key.

## Automatic Bash test

Run the arming commands and wait for the prompt to return:

```sh
shellbell off
shellbell name "WSL Bash test"
shellbell once --after 5s --idle 5s --to pc
```

Then submit the monitored command from the next prompt:

```sh
sleep 7
```

Do not type another command during the five-second settling interval. The expected notification is:

```text
WSL Bash test is ready
```

Pasting the arming command and monitored command in one multiline prompt submission cannot be measured retroactively. Shellbell observes prompt-to-prompt shell boundaries, so monitoring begins with the next command submitted after the arming prompt returns.

After delivery:

```sh
shellbell status
```

should report `Mode: off` and `State: disarmed` for once mode.

## Notification troubleshooting

A ring appearing in the PWA proves the source-to-relay path worked, but it does not prove a visible operating-system banner was shown.

Check these layers independently:

1. The receiver is registered and enabled with a matching target tag.
2. The relay ring row reports `matched_receivers: 1`.
3. The delivery row reports `delivered` with no diagnostic.
4. Browser permission for `shellbell.ashishajin.com` is allowed.
5. Browser notifications are enabled in the operating system.
6. Do Not Disturb or fullscreen notification suppression is disabled when testing.

On Windows, Chrome site permission and Windows notification permission for Google Chrome are separate controls. A delivery can be accepted by the Push provider while Windows suppresses the visible banner.

When inspecting delivery records, print only receiver names, tags, delivery status, diagnostics, and timestamps. Do not print Push endpoints or encryption keys.

## WSL daemon startup

A WSL distribution without a usable systemd user manager uses shell-start lazy activation. The managed hook starts the per-user daemon when required. `shellbell doctor` may warn that the systemd user manager is unavailable while still reporting a healthy daemon, IPC socket, source token, and durable queue.

## Privacy boundary

The production deployment retains notification messages and privacy-safe routing metadata. It does not collect command text, command output, exit codes, current directories, environment variables, process lists, or inferred success/failure state.
