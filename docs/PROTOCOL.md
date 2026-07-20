# HTTP protocol

Shell activity metadata stays local. Automatic and manual delivery use the same idempotent request:

```json
{"event_id":"uuid","message":"Shell is ready","target_tags":["phone"]}
```

No shell PID, TTY, shell type, state, command, output, exit code, environment, process data, or active duration is sent to the relay.

All API bodies are JSON, timestamps are UTC RFC 3339 strings, and HTTP requests are capped at 16 KiB. Errors use:

```json
{"error":{"code":"validation_error","message":"name must not be empty"}}
```

Common codes are `validation_error` (400), `unauthorized` (401), `forbidden` (403), `not_found` (404), `conflict` (409), `rate_limited` (429), and `internal_error` (500).

## Authentication

- Public endpoints need no credential.
- Owner endpoints require the `sb_session` cookie; mutations also require the readable `sb_csrf` value in `X-CSRF-Token`.
- Source endpoints require `Authorization: Bearer <source-token>`. A source can only submit rings and fetch itself.

Raw source/session tokens are not stored by the relay. Source revocation is immediate. The per-user daemon reads the separately protected local source credential only in its delivery worker.

## Validation limits

- Names and session labels: trimmed, 1–64 Unicode scalar values, no controls.
- Ring message: optional, trimmed, at most 240 Unicode scalar values; newline/tab allowed, other controls rejected.
- Receiver targets: unique `phone`, `pc`, `mobile`, or `desktop`; an empty target list means all enabled receivers.
- Push endpoint: HTTPS and at most 2048 bytes; Push keys are required and at most 512 bytes each.
- Pairing expires after 10 minutes. Ring rate is 120/minute/source in the relay process.
- Ring history `limit` defaults to 100 and is clamped to 1–200.

## Endpoints

| Method | Path | Auth | Purpose |
|---|---|---|---|
| `GET` | `/health` | Public | relay health/version |
| `GET` | `/api/owner/bootstrap/status` | Public | initial owner state |
| `POST` | `/api/owner/bootstrap` | Public | bootstrap owner and issue cookies |
| `POST`, `GET` | `/api/owner/session` | Public / Owner | sign in / validate session |
| `POST` | `/api/owner/logout` | Owner + CSRF | revoke owner session |
| `POST`, `GET` | `/api/pairings` | Public / Owner | create / list pairing requests |
| `GET` | `/api/pairings/{id}` | Public, unguessable ID | poll pairing |
| `POST` | `/api/pairings/{id}/approve` | Owner + CSRF | approve once |
| `POST` | `/api/pairings/{id}/reject` | Owner + CSRF | reject pending request |
| `GET` | `/api/sources/self` | Source | validate source credential |
| `GET` | `/api/sources` | Owner | list sources |
| `PATCH`, `DELETE` | `/api/sources/{id}` | Owner + CSRF | rename / revoke source |
| `POST`, `GET` | `/api/receivers` | Owner + CSRF / Owner | register / list receivers |
| `PATCH`, `DELETE` | `/api/receivers/{id}` | Owner + CSRF | update / revoke receiver |
| `POST` | `/api/rings` | Source | idempotent ring submission |
| `GET` | `/api/rings?limit=100` | Owner | newest ring history |
| `GET`, `PUT` | `/api/settings` | Owner / Owner + CSRF | retention and VAPID public key |

## Ring idempotency

A new ring returns `202`:

```json
{"event_id":"uuid","accepted_at":"2026-01-01T00:00:00Z","duplicate":false,"matched_receivers":1}
```

The relay commits the ring and matched delivery rows under `UNIQUE(source_id,event_id)` before Push attempts. Repeating the exact event ID returns `200` with `duplicate:true`, the original accepted time/count, and no new history or Push. This is the only request retried by the local daemon.

`RingView` remains exactly `event_id`, `source_id`, `source_name`, optional `message`, `created_at`, and `target_tags`. See [LOCAL_IPC.md](LOCAL_IPC.md) for the local-only protocol.
