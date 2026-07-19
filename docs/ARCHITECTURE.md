# Architecture

Milestone 1 is one manual notification vertical slice. The `shellbell` CLI creates a pairing request or submits a privacy-limited ring. The Axum relay authenticates owners and sources, persists state in SQLite, routes accepted rings to matching browser receivers, and serves the compiled React PWA. A Caddy reverse proxy terminates production TLS.

## Components

- `shellbell-protocol` owns JSON request/response shapes, length limits, tag validation, and serialization tests.
- `shellbell-core` owns random token/code generation, SHA-256 credential hashing, constant-time verification, and redaction helpers.
- `shellbell-cli` stores one relay/source credential in the platform per-user config directory. It supports Linux and WSL now while using portable path and HTTP libraries.
- `shellbell-relay` owns Axum routes, authentication/CSRF, SQLx transactions and migrations, rate limits, receiver routing, retention, Web Push, and static PWA delivery.
- `pwa` is a React/TypeScript/Vite installable application with an explicit Push permission flow and service worker.

## Trust and data flow

The owner bootstrap token is a root secret supplied by environment or generated into `/data/owner-bootstrap-token` with mode `0600`. Successful bootstrap or sign-in creates a random opaque session. Only its SHA-256 hash is stored. The browser receives an HttpOnly `SameSite=Strict` session cookie and a separate CSRF cookie; mutations require the matching `X-CSRF-Token` header.

Pairing requests are public, short-lived, rate-limited records. Owner approval transactionally creates a source with no credential. The first approved poll generates a send-only credential, stores only its hash, and returns the raw value once. Rings are inserted under `UNIQUE(source_id,event_id)` in a transaction. Delivery begins only after commit, so retries cannot create history or Push duplicates.

Push delivery is the `PushDelivery` interface. Production uses VAPID and an eight-second bound per receiver. Tests use an in-memory fake. A permanent endpoint/key failure disables its receiver; transient failure retains it. Delivery continues receiver by receiver even when an earlier attempt fails.

## Privacy boundary

The ring schema contains event ID, source ID and display-name snapshot, optional owner-written message, timestamp, and target tags. Shellbell has no field or code path for commands, output, exit status, process information, environment, working directory, shell identity, or inferred success/failure. Milestone 1 deliberately has no shell hooks or automatic monitoring.
