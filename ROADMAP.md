# Roadmap

Shellbell `v0.1.0` is released and running in production. The project remains focused on a small, reliable, privacy-preserving terminal notifier that is easy to self-host.

## Completed: v0.1.0

- Open-source readiness and public-content privacy checks.
- Linux x86-64 and ARM64 release archives with checksums.
- Multi-architecture immutable container publishing.
- Upgrade and rollback documentation.
- End-to-end manual and automatic notification validation.

## Milestone 5: onboarding and operations

- Checksum-verifying installer and upgrade path for Linux x86-64 and ARM64.
- Guided static setup site with runnable Linux, WSL, and Raspberry Pi 5 commands.
- Raspberry Pi 5 installation and acceptance guide.
- Consistent relay backup helper with checksum manifests.
- Restore helper with a pre-restore safety snapshot and automatic rollback.
- Safe source diagnostics and delivery-test workflow.
- Repeatable full-history privacy-audit helper with local private-pattern support.

## Later

Likely improvements, based on real usage:

- Native PowerShell, Windows, and macOS support.
- Package-manager distribution where maintenance cost is justified.
- Richer receiver and delivery diagnostics that preserve Push-secret boundaries.
- A native mobile receiver only if browser Push reliability proves insufficient.

## Non-goals

Shellbell is an attention notifier, not a job manager or terminal recorder. The project does not plan to collect command text, output, environment variables, process lists, working directories, or inferred command outcomes.
