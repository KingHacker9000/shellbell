# Roadmap

Shellbell is approaching its first public release. The immediate goal is a small, reliable, privacy-preserving terminal notifier that is easy to self-host.

## v0.1.0

- Finish the open-source readiness audit.
- Validate clean installation on supported Linux and WSL environments.
- Publish Linux x86-64 and ARM64 release archives with checksums.
- Publish an immutable container image.
- Document upgrade and rollback steps.
- Test manual rings, automatic monitoring, retry recovery, and browser notifications end to end.

## After v0.1.0

Likely improvements, based on real usage:

- Easier installation and upgrade packages.
- A small documentation site with guided setup and runnable examples.
- Backup and restore helpers for self-hosted relays.
- Better receiver and delivery diagnostics without exposing Push secrets.
- Native PowerShell, Windows, and macOS support.
- A native mobile receiver only if browser Push reliability proves insufficient.

## Non-goals

Shellbell is an attention notifier, not a job manager or terminal recorder. The project does not plan to collect command text, output, environment variables, process lists, working directories, or inferred command outcomes.
