# HTTP protocol

All API bodies are JSON and timestamps are UTC RFC 3339 strings. Unknown fields are ignored by Serde. Requests are limited to 16 KiB. Errors use:

```json
{"error":{"code":"validation_error","message":"name must not be empty"}}
```

Common codes are `validation_error` (400), `unauthorized` (401), `forbidden` (403), `not_found` (404), `conflict` (409), `rate_limited` (429), and `internal_error` (500). No response includes command content, output, status, environment, directories, process data, or shell data.

## Authentication

- **Public** endpoints need no credential.
- **Owner** endpoints require the `sb_session` cookie. Owner mutations also require the readable `sb_csrf` cookie value in `X-CSRF-Token`. Cookies are `SameSite=Strict`, expire after 24 hours, and are `Secure` outside documented localhost mode.
- **Source** endpoints require `Authorization: Bearer <source-token>`. The source token can access only ring submission and source-self.

Raw source and session tokens are not stored. Logout and source revocation take effect immediately. The bootstrap token is the single-owner recovery credential and is submitted only in the two session-establishing bodies.

## Limits and enums

- Names: trimmed, 1–64 Unicode scalar values, no control characters.
- Optional ring message: trimmed, 0–240 Unicode scalar values; tab/newline are allowed but other controls are not.
- Receiver tags: unique values from `phone`, `pc`, `mobile`, `desktop`. An empty ring target list means all enabled receivers.
- Push endpoint: valid HTTPS URL, at most 2048 bytes. `p256dh` and `auth`: required, each at most 512 bytes.
- History retention: integer 1–90 days; default 14.
- Pairing: expires 10 minutes after creation. Codes have eight unambiguous characters rendered `XXXX-XXXX`.
- Ring history query: `limit` defaults to 100 and is clamped to 1–200.

## Endpoints

### Service and owner

| Method | Path | Auth | Request | Success response |
|---|---|---|---|---|
| `GET` | `/health` | Public | — | `200 {"status":"ok","version":"0.1.0"}` |
| `GET` | `/api/owner/bootstrap/status` | Public | — | `200 {"bootstrap_required":true}` |
| `POST` | `/api/owner/bootstrap` | Public | `{"bootstrap_token":"..."}` | `200 SessionResponse` plus cookies; `409` if already bootstrapped |
| `POST` | `/api/owner/session` | Public | `{"bootstrap_token":"..."}` | `200 SessionResponse` plus new cookies; `409` if bootstrap is incomplete |
| `GET` | `/api/owner/session` | Owner | — | `200 {"authenticated":true,"expires_at":"..."}` |
| `POST` | `/api/owner/logout` | Owner + CSRF | `{}` or empty JSON body | `204`, session revoked and cookies cleared |

An invalid bootstrap token returns `401`. Bootstrap completion is single-owner and single-use, but the configured high-entropy bootstrap token remains usable to create a fresh revocable session.

### Pairing

| Method | Path | Auth | Request | Success response |
|---|---|---|---|---|
| `POST` | `/api/pairings` | Public, rate limited | `{"display_name":"laptop"}` | `201 {"id":"uuid","code":"ABCD-2345","expires_at":"..."}` |
| `GET` | `/api/pairings/{id}` | Public, unguessable ID | — | `200 PairingPollResponse` |
| `GET` | `/api/pairings` | Owner | — | `200 {"pairings":[PairingView...]}` (pending and unexpired only) |
| `POST` | `/api/pairings/{id}/approve` | Owner + CSRF | `{}` | `200 SourceView`; `409` unless pending and unexpired |
| `POST` | `/api/pairings/{id}/reject` | Owner + CSRF | `{}` | `204`; `409` unless pending |

`PairingPollResponse.status` is `pending`, `approved`, `rejected`, or `expired`. Approval transactionally creates a source. The first poll after approval also returns `source_id` and `source_token`. The raw token is returned once; later safe repeated polls remain `approved` but omit `source_token`. Rejected and expired pairings cannot transition to approved. Simultaneous decisions produce one success and a conflict for the loser.

### Sources

| Method | Path | Auth | Request | Success response |
|---|---|---|---|---|
| `GET` | `/api/sources/self` | Source | — | `200 SourceView` |
| `GET` | `/api/sources` | Owner | — | `200 {"sources":[SourceView...]}` |
| `PATCH` | `/api/sources/{id}` | Owner + CSRF | `{"name":"work laptop"}` | `200 SourceView` |
| `DELETE` | `/api/sources/{id}` | Owner + CSRF | — | `204`; subsequent source authentication fails |

`SourceView` contains `id`, `display_name`, `created_at`, optional `last_seen_at`, and optional `revoked_at`. It never exposes token material.

### Push receivers

| Method | Path | Auth | Request | Success response |
|---|---|---|---|---|
| `POST` | `/api/receivers` | Owner + CSRF | `ReceiverCreateRequest` below | `201 ReceiverView`; an existing endpoint is refreshed in place |
| `GET` | `/api/receivers` | Owner | — | `200 {"receivers":[ReceiverView...]}` |
| `PATCH` | `/api/receivers/{id}` | Owner + CSRF | any of `name`, `tags`, `enabled` | `200 ReceiverView` |
| `DELETE` | `/api/receivers/{id}` | Owner + CSRF | — | `204` |

Registration body:

```json
{
  "name": "Phone browser",
  "tags": ["phone", "mobile"],
  "subscription": {
    "endpoint": "https://push-provider.example/opaque",
    "p256dh": "browser-public-key",
    "auth": "browser-auth-secret"
  }
}
```

`ReceiverView` contains only `id`, `name`, tags, enabled state, and creation time. Subscription material is never returned by list/update responses. Revocation is soft deletion. A permanent Push failure changes `enabled` to false.

### Rings

| Method | Path | Auth | Request | Success response |
|---|---|---|---|---|
| `POST` | `/api/rings` | Source, rate limited | `RingRequest` | `202 RingAcceptedResponse` for a new event; `200` for a duplicate |
| `GET` | `/api/rings?limit=100` | Owner | — | `200 {"rings":[RingView...]}` newest first |

Request and response:

```json
{"event_id":"uuid","message":"Test message","target_tags":["pc"]}
```

```json
{"event_id":"uuid","accepted_at":"2026-01-01T00:00:00Z","duplicate":false,"matched_receivers":1}
```

Idempotency is scoped to `(source_id,event_id)`. The relay transaction checks/inserts that unique key and snapshots the matched receiver count. Repeating it returns the original timestamp/count with `duplicate:true`; it creates no history or delivery records and sends no Push. A new accepted ring is committed before delivery attempts. All matching enabled receivers are attempted even when another fails. Only attempt time, outcome class, and a non-secret diagnostic are retained.

`RingView` has exactly `event_id`, `source_id`, `source_name`, optional `message`, `created_at`, and `target_tags`.

### Settings

| Method | Path | Auth | Request | Success response |
|---|---|---|---|---|
| `GET` | `/api/settings` | Owner | — | `200 {"history_retention_days":14,"vapid_public_key":"..."}` |
| `PUT` | `/api/settings` | Owner + CSRF | `{"history_retention_days":14}` | `200` with the same response shape |

Retention cleanup runs during ring acceptance and deletes rings older than the configured number of days; deliveries cascade. Sources and receiver records remain until explicitly revoked, and revoked records remain as an audit-light identity record. Pairing records are retained with their terminal status but contain no raw credential.
