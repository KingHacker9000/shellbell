# Milestones

## Milestone 1 — manual vertical slice (complete)

Owner bootstrap/session, source pairing and revocation, manual CLI rings, receiver tags, VAPID Web Push, idempotent ring history, installable PWA, relay SQLite, Docker Compose, Caddy example, and security tests remain supported.

## Milestone 2 — Linux and WSL shell activity monitoring (complete)

- Linux x86-64, Linux ARM64, and WSL 2 client support.
- Interactive Bash, Zsh, and Fish hooks with stable nested-session UUIDs.
- Persistent and once arming, monotonic cumulative foreground activity, settling timers, session labels/targets, manual duplicate suppression, and concurrent sessions.
- Same-binary per-user daemon, UID-checked bounded Unix IPC, secure XDG runtime path/fallback, SQLite recovery, stale-session expiry, durable idempotent retry queue, authorization surfacing, and clean shutdown.
- Managed idempotent install/uninstall, backups, systemd user service, and no-systemd lazy activation without root or `/etc/wsl.conf` edits.
- Strict user TOML separate from existing paired source credentials.
- Deterministic state/store/IPC tests, isolated Bash/Zsh/Fish harnesses, WSL/service tests, Linux CI, x86-64 tests, ARM64 compile check, PWA/Docker preservation, and complete operational docs.

## Milestone 3 — private production deployment (acceptance complete)

- Private relay and PWA deployed at `https://shellbell.ashishajin.com` on AWS Lightsail.
- Docker Compose stack under `/opt/stacks/shellbell`, localhost-only backend on `127.0.0.1:8080`, and Caddy as the only public ingress.
- Persistent SQLite data, external mode-`0600` secrets, immutable commit-tagged images, bounded resources/logs, and health checks.
- Owner bootstrap, browser receiver registration, WSL source pairing, manual Push delivery, durable retry recovery, and automatic Bash once-mode delivery verified end to end.
- Browser and operating-system notification controls verified as separate delivery layers.
- Production acceptance and troubleshooting are documented in `docs/PRODUCTION_ACCEPTANCE.md`.

## Later milestones

- Windows PowerShell/CMD and macOS support.
- Packaging/upgrades, backup tooling, native mobile applications, and private infrastructure operations.
- Carefully privacy-reviewed delivery observability beyond current local queue status.

Shellbell remains an attention notifier, not a job manager. Job/process tracking, output-silence detection, prompt scraping, outcome inference, and application-specific monitoring are outside the current milestones.
