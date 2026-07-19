# Security and threat model

Shellbell is a private single-owner service exposed through HTTPS. The protected assets are the owner session, source send capability, Push subscription material, ring history, and SQLite database. Likely threats are internet scanning and pairing spam, stolen browser/source credentials, CSRF, replayed ring submissions, malicious oversized input, accidental secret logging, and a leaked database backup.

## Controls

- Owner bootstrap/source/session credentials use operating-system randomness. Raw source and owner-session tokens are never stored in SQLite; SHA-256 hashes are verified in constant time.
- The bootstrap token is accepted from an environment secret or generated once into a user-readable-only data file. It is never logged. Treat it as a recovery/sign-in secret and rotate it by replacing the environment value and revoking sessions.
- Owner cookies are HttpOnly, `SameSite=Strict`, path-scoped, expiring, and `Secure` unless explicit localhost mode is enabled. Mutations additionally require a double-submit CSRF header/cookie match.
- Source credentials authorize only `POST /api/rings` and `GET /api/sources/self`. Revocation nulls the hash immediately.
- Pairing approval/rejection and one-time credential issuance are transactional. Pairings expire after 10 minutes. Codes omit `0`, `1`, `I`, and `O`.
- Event IDs are unique per source. The database transaction suppresses duplicate history and delivery.
- JSON bodies are capped at 16 KiB. Names, messages, tags, retention, URLs, and Push key sizes are validated. Pair creation is limited to 12/minute/client and rings to 120/minute/source in-process.
- Logs contain route/status context and redacted diagnostics, never authorization headers, request bodies, Push endpoints, or full credentials.
- Web Push has bounded delivery time. Permanent subscription failures disable the receiver; transient failures do not block other receivers.
- SQLite enables foreign keys, WAL, constraints, and indexes. Mount `/data` as owner-only (`0700`) and back it up as sensitive data.

## Deployment assumptions and residual risk

Production relies on Caddy to redirect HTTP, terminate current TLS, and supply the proxy boundary. Set `SHELLBELL_INSECURE_LOCAL_HTTP=false` so cookies are `Secure`; do not publish relay port 8080 directly. The in-memory rate limiter resets at restart and is intentionally modest for one owner. Push subscription keys are stored in SQLite because delivery requires them; filesystem isolation and encrypted host backups protect them. The bootstrap token remains a reusable recovery credential in Milestone 1, so it must be high entropy and stored like a password.

## Pre-commit secret review

Before every release, inspect staged files and scan for `sb_owner_`, `sb_session_`, `sb_src_`, bearer headers, VAPID private values, Push endpoints, `.env` files, and SQLite files. `.gitignore` excludes common secret/data artifacts; examples contain placeholders only.
