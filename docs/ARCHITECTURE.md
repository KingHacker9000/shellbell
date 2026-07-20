# Architecture

Shellbell combines a privacy-bounded per-user activity monitor with a private notification relay and browser receivers.

## Components

- `shellbell-protocol` owns the backwards-compatible relay JSON API, validation, limits, and serialization tests.
- `shellbell-core` owns random token/code generation, hashing, and constant-time credential verification.
- `shellbell-local` owns strict TOML settings, typed local IPC, the activity state machine, durable local SQLite state/queue, UID-checked daemon, managed shell hooks, systemd unit generation, and WSL detection.
- `shellbell-cli` owns pairing and all public user commands. The same executable exposes hidden `__daemon`, `__hook`, and `__session-id` modes; no second binary is installed.
- `shellbell-relay` owns Axum routes, owner/source authentication, relay SQLite, receiver routing, idempotent ring acceptance, Web Push, and PWA delivery.
- `pwa` is the React/TypeScript/Vite owner interface.

## Local data flow

Every integrated interactive shell creates a random UUID, exports it as `SHELLBELL_SESSION_ID`, and exports the safe shell type. Its managed hook sends only boundary events and local metadata over a Unix socket. The daemon verifies Linux peer credentials against its effective UID before parsing an 8 KiB-bounded, versioned JSON message.

The daemon keeps one in-memory state machine per shell session. Live foreground durations and deadlines use a monotonic clock. Durable SQLite contains the session metadata needed for safe restart and unsent ring records keyed by their original event UUID. The shell hook never makes a network request or waits for delivery.

The delivery worker sends at most one due queued event per second to `POST /api/rings`. Network, `429`, and `5xx` failures retain the event UUID and receive exponential backoff. `401`/`403` becomes a non-retrying authorization-failure record visible to `doctor`. Relay uniqueness on `(source_id,event_id)` makes a lost-response retry safe: an already accepted event produces no second Push.

## State machine

The public model is:

```text
DISARMED --on/once--> ARMED --command_start--> ACTIVE
    ^                    ^                         |
    |                    |                         | prompt_ready
    |                    +---- deadline/reset <---v
    +------- off ----------------------------- SETTLING
                                      command_start |
                                      cancels timer +--> ACTIVE
```

`NOTIFIED` is represented by `burst_notified` while ACTIVE or SETTLING timing remains intact.

- `command_start` in ACTIVE is a duplicate and does not restart or double-count time.
- `prompt_ready` adds `now - command_started` to accumulated foreground wall time and creates one settling deadline. A duplicate prompt event does not extend it.
- A new command in SETTLING cancels the deadline, returns to ACTIVE, and retains accumulated time.
- At the deadline, a qualifying unnotified burst enqueues one automatic ring. A short burst resets silently.
- Persistent mode returns to ARMED. Once mode disarms only after a qualifying automatic ring or a manual ring associated with a live burst; a short burst leaves once mode armed.
- A manual ring marks the active burst notified and suppresses its automatic duplicate. A standalone manual ring does not consume persistent mode.
- `off` clears the burst and deadline immediately.

## Restart and staleness

Settling state stores a wall-clock deadline only for reconstructing the remaining timer after restart; normal live timing is monotonic. A daemon cannot know when a shell regained foreground control while it was down, so an ACTIVE burst is discarded and its persistent/once mode is safely re-armed. It is never treated as completed. Settling bursts can resume and retain their queued-event idempotency guarantees.

Sessions are removed when their shell PID no longer exists or their last local boundary is older than 24 hours. PID, TTY, shell type, modes, labels, and state are local only and never added to relay ring payloads.

## Service lifecycle

`shellbell install` writes a bounded-restart systemd user unit when a usable user manager exists, then runs `systemctl --user enable --now shellbell.service`. The unit is architecture-neutral and runs the installed executable in `__daemon` mode. Without systemd, managed hooks lazily start the detached per-user daemon on first use. Concurrent lazy starts converge on the single Unix socket. Shutdown removes the socket cleanly.

## Relay trust path

Owner cookies, CSRF, send-only source credentials, transactional pairing, tagged receiver selection, relay idempotency, and PWA behavior share one privacy boundary. The relay `RingRequest` contains only an event ID, calm notification message, and target tags. Active duration and shell metadata are intentionally not transmitted.
