# Production deployment acceptance

This document provides a provider-neutral acceptance procedure for a private
Shellbell deployment.

Example URL:

```text
https://shellbell.example.com
```

Replace the example domain with the deployment's real domain.

## Reference layout

A typical deployment uses:

- Stack directory: `/opt/stacks/shellbell`
- Compose file: `/opt/stacks/shellbell/compose.yaml`
- Secret environment file: `/opt/stacks/shellbell/.env` with mode `0600`
- Persistent SQLite data: `/opt/stacks/shellbell/data`
- Container name: `shellbell`
- Backend binding: `127.0.0.1:8080`
- Public ingress: a reviewed HTTPS reverse proxy on ports 80 and 443
- Immutable image tags derived from a release or source revision
- Bounded memory, process count, and container logs
- Local and public health checks

Port 8080 must remain private.

Before replacing deployment files or persistent data, create timestamped
backups and validate both Docker Compose and the complete reverse-proxy
configuration.

## End-to-end acceptance path

1. Bootstrap the owner session in the PWA.
2. Register a browser receiver and grant site notification permission.
3. Pair a source with:

   ```sh
   shellbell pair https://shellbell.example.com
   ```

4. Send a manual notification with `shellbell ring`.
5. Verify the local durable queue retries a temporary relay failure without
   changing the event ID.
6. Verify the relay records a matching receiver and successful Push delivery.
7. Install a supported shell hook.
8. Open a new interactive shell.
9. Arm the shell and run a qualifying foreground command.
10. Verify delivery after the command returns and the idle window expires.
11. Verify `once` mode returns to `off` and `disarmed`.

## Manual notification test

Register a receiver carrying the `pc` tag:

```sh
shellbell ring "Shellbell manual test" --to pc
```

`Ring queued` means the local daemon accepted the event. Submission to the
relay occurs asynchronously. During an outage, the original event remains in
the durable local queue and retains its idempotency key.

## Automatic Bash test

The monitored command may be submitted from the next prompt:

```sh
shellbell off
shellbell name "Bash test"
shellbell once --after 5s --idle 5s --to pc
```

Then:

```sh
sleep 7
```

It may also follow the successful arming command in the same pasted block:

```sh
shellbell once --after 5s --idle 5s --to pc
sleep 7
```

The daemon atomically arms the session and starts a fresh local activity
boundary. It does not retroactively count commands that ran before the
`on` or `once` operation succeeded.

Do not submit another command during the settling interval. The expected
notification is:

```text
Bash test is ready
```

After delivery, `shellbell status` should report `Mode: off` and
`State: disarmed` for once mode.

## Notification troubleshooting

A ring appearing in the PWA proves that the source-to-relay path worked. It
does not prove that an operating-system banner was displayed.

Check these layers independently:

1. The receiver is registered, enabled, and has a matching target tag.
2. The relay ring reports at least one matched receiver.
3. The delivery record reports `delivered` without a diagnostic.
4. Browser permission for the deployment domain is allowed.
5. Browser notifications are enabled by the operating system.
6. Do Not Disturb and fullscreen notification suppression are disabled while
   testing.

For example, Chrome site permission and Windows notification permission for
Chrome are separate controls. A Push provider may accept a delivery while the
operating system suppresses the visible banner.

When inspecting delivery records, print only receiver names, tags, delivery
status, diagnostics, and timestamps. Do not print Push endpoints or encryption
keys.

## WSL daemon startup

A WSL distribution without a usable systemd user manager uses shell-start lazy
activation. `shellbell doctor` may warn that the user manager is unavailable
while still reporting a healthy daemon, IPC socket, source token, and durable
queue.

## Privacy boundary

The relay retains notification messages and privacy-safe routing metadata. It
does not collect monitored command text, command output, exit codes, current
directories, environment variables, process lists, or inferred success or
failure.
