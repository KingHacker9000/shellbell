# Local IPC and durable state

The per-user daemon listens on a Unix-domain socket:

- `$SHELLBELL_RUNTIME_DIR/daemon.sock` when the explicit test/development override is set.
- `$XDG_RUNTIME_DIR/shellbell/daemon.sock` when the XDG directory is owned by the user and not group/other writable.
- `/tmp/shellbell-<uid>/daemon.sock` as the secure Linux/WSL fallback.

The containing directory is `0700`, the socket is `0600`, and every accepted stream's peer UID must equal the daemon effective UID.

## Framing and limits

IPC is one JSON object followed by newline, then one JSON response and connection close. Both directions are capped at 8192 bytes. Hook calls use a 90 ms operation timeout and fail open. Explicit commands use a two-second timeout and return an actionable error. The server applies a 250 ms request-read timeout.

Every request has:

```json
{
  "version": 1,
  "request_id": "uuid",
  "type": "daemon_ping"
}
```

Unknown versions, types, and fields are rejected. The response repeats `version` and `request_id`, contains `ok`, and contains either typed `result` or a safe local error. No schema contains command text, command output, exit status, working directory, environment, shell history, or a process list.

## Request types

| Type | Fields beyond envelope | Purpose |
|---|---|---|
| `session_open` | `session_id`, `shell_pid`, `shell_type`, optional `tty` | register/update integrated shell |
| `command_start` | `session_id` | foreground control left the prompt |
| `prompt_ready` | `session_id` | foreground control returned |
| `session_close` | `session_id` | remove session |
| `arm_persistent` | `session_id`, `minimum_active_ms`, `idle_for_ms`, `targets` | arm future bursts |
| `arm_once` | same | arm one qualifying notification |
| `disarm` | `session_id` | reset/cancel timer |
| `set_label` | `session_id`, nullable `label` | session-only display label |
| `status` | `session_id` | safe local status |
| `manual_ring` | optional `session_id`, optional `message`, `targets` | associate/suppress and enqueue |
| `daemon_ping` | none | queue/session/authorization counters |
| `daemon_shutdown` | none | same-user clean fallback shutdown |

Shell types are `bash`, `zsh`, and `fish`. Targets use the relay's existing validation; an empty list means all. Labels and messages use the existing 64/240-character limits. Durations are bounded before entering the state machine.

## Responses

`acknowledged` confirms a local mutation. `status` returns the session UUID, local shell PID/type/TTY, mode/state/burst ID, accumulated active milliseconds, optional wall deadline, notification flag, label, targets, and last-seen time. `ring_queued` returns the durable event UUID, or null when a second manual ring for the current burst was suppressed. `pong` returns pending events, authorization failures, policy drops, and active-session count.

None of this status payload goes to the relay.

## Persistence

Linux stores local state at `$XDG_DATA_HOME/shellbell/activity.db` (normally `~/.local/share/shellbell/activity.db`) unless `SHELLBELL_STATE_DIR` is explicitly set for development/tests. SQLite uses WAL and one local connection.

The session table stores only recovery metadata. Monotonic timestamps are never serialized. ACTIVE state is reset to ARMED on restart because foreground return time is unknowable. A SETTLING wall deadline is converted back to a monotonic remaining delay and can enqueue the event once.

Queue rows contain original event UUID, optional message, target tags, creation time, attempt count, next-attempt time, status, and a bounded diagnostic. Accepted rows are deleted. The default capacity is 100 and maximum age 24 hours. The oldest row is evicted at capacity. Expired/auth-failed rows do not create an offline flood.

Retries are limited to the existing idempotent ring submission. The original UUID is never regenerated. Transient delay starts at five seconds, doubles to a 15-minute base cap, and adds at most 25% jitter. Permanent authentication failures stop retrying and are reported by `shellbell doctor`.
