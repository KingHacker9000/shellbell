# Security, privacy, and threat model

Shellbell protects the owner session, source send capability, browser Push subscription, local shell-session metadata, retry queue, and both SQLite databases. Threats include internet scanning, stolen credentials, CSRF, replay, malformed local/HTTP input, cross-user socket access, symlink/file clobbering, accidental command capture, and notification flooding after an outage.

## Privacy boundary

Shell hooks observe only these boundaries: shell opened, foreground command began, prompt became ready, and shell exited. They do not inspect hook arguments carrying command text. There is no command/output/status/environment/history/process-list schema in IPC or durable state.

Local-only session data can include UUID, shell PID/type, TTY, arm mode/state, burst UUID, accumulated duration, deadline, notified flag, label, receiver targets, and last-seen time. The daemon uses shell PID only for stale cleanup. Relay RingRequest remains event UUID, optional deliberate/calm message, and target tags. Internal shell metadata and duration are not transmitted.

## Local controls

- Runtime/config/state directories are forced to `0700`; credentials, local SQLite, hooks, unit, and socket are `0600`.
- `$XDG_RUNTIME_DIR` is used only when it is a directory owned by the effective UID and not group/other writable. The fallback is `/tmp/shellbell-<uid>`, created `0700`.
- The server obtains `SO_PEERCRED` through the Unix stream and rejects a UID other than its effective UID.
- IPC is JSON-line, version 1, capped at 8 KiB, strict about known fields, validated, and timeout-bounded.
- Hook errors and daemon absence fail open so prompt rendering is never dependent on Shellbell.
- Managed startup changes are one marked line. Existing files receive a timestamped backup, append-only install, preserved permissions/line endings, and targeted uninstall. Alternate-home tests protect the developer home.
- Configuration and credential writes are same-directory temporary-file plus fsync/rename operations.

## Queue and replay controls

The default queue retains at most 100 events and events at most 24 hours. At capacity the oldest event is dropped and the counter is reported by `doctor`. Expired events are deleted before selection/enqueue. Delivery processes one due event per second, preventing an offline backlog flood.

Transient network, `429`, and `5xx` failures use exponential delay from five seconds, capped at 15 minutes before up to 25% jitter. The original event UUID never changes. `401`/`403` is marked `authorization_failure` and never retried forever; it remains locally visible until age expiry or explicit purge. Other permanent 4xx responses are discarded and counted. The relay's `(source_id,event_id)` uniqueness makes response-loss retry at-most-once for remote Push acceptance.

## Relay controls

- Credentials use OS randomness; only source/session hashes are stored by the relay and verified in constant time.
- Owner cookies are HttpOnly, `SameSite=Strict`, expiring, `Secure` outside explicit localhost mode, and mutations require CSRF matching.
- Pairing decisions and credential issuance are transactional and short-lived.
- HTTP bodies/fields and request rates are bounded. Source capabilities are send-only and revocable.
- Logs omit authorization, request bodies, Push endpoints, raw credentials, and local session metadata.
- SQLite uses foreign keys, WAL, constraints, and indexed idempotency. Relay `/data` must remain owner-only and backups are sensitive.

## Residual risks and limitations

Shell startup files execute code and remain part of the user's trust boundary. A malicious same-user process can access same-user files/socket and use the source send capability; Unix UID separation is not a sandbox between processes of one account. A ring may occur while text is being typed because Milestone 2 deliberately avoids Readline/ZLE/Fish-editor interception. Background processes and interactive applications holding foreground control are not completion signals. The relay in-memory rate limiter resets on relay restart.

## Pre-commit review

Inspect the full/staged diff and untracked files; run `git diff --check`; reject private-key headers and GitHub-token-shaped values; verify no `.env`, SQLite, socket, PID, generated hook, or package-extraction artifact is staged; and search for shell command-content capture such as `BASH_COMMAND`, hook argument serialization, output, exit status, or environment enumeration. Placeholder strings in tests are not credentials.
