# Changelog

All notable changes to Shellbell are documented here.

Shellbell follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Checksum-verifying Linux installer and upgrade path.
- Guided static setup site for Linux, WSL 2, and Raspberry Pi 5.
- Relay backup and restore helpers with safety snapshots and checksum manifests.
- Safe source diagnostics and full-history privacy-audit helpers.
- Raspberry Pi 5 setup and acceptance documentation.

### Changed

- Release automation now publishes the installer and includes it in `SHA256SUMS`.
- CI validates installer upgrades, checksum rejection, operational scripts, and the documentation site.
- Linux CLI archives are built on the GLIBC 2.35 baseline for Ubuntu 22.04, compatible ARM64 Linux, and Raspberry Pi 5 systems.

### Fixed

- The installer now executes and validates the downloaded binary before atomically replacing an existing installation.
- CI rejects release binaries that require a newer GLIBC than the supported baseline.
- History and site privacy checks no longer report their own detector definitions as findings.

## [0.1.0] - 2026-07-20

First stable public release.

### Added

- Private single-owner relay and installable PWA.
- Manual terminal notifications.
- Opt-in automatic monitoring for Bash, Zsh, and Fish.
- Linux x86-64, Linux ARM64, Raspberry Pi OS, and WSL 2 support.
- Per-user daemon with bounded Unix socket IPC.
- Durable local retry queue and idempotent relay delivery.
- Tagged Web Push receivers.
- High-urgency Web Push delivery with a short notification TTL.
- Docker deployment with persistent SQLite storage.
- Security, privacy, protocol, architecture, configuration, operations, and release documentation.
- Open-source contribution guides and issue templates.
- Reproducible Linux CLI archives, checksums, GitHub releases, and multi-architecture container publishing.

### Changed

- Simplified public documentation for first-time users.
- Reframed internal milestone notes as a public roadmap and operations guide.
- Filtered release artifacts so Docker build records cannot block GitHub release publishing.
- Added Buildx caching and a bounded container-build timeout for future releases.

### Validated

- Linux x86-64 and ARM64 CLI archives passed checksum verification.
- The multi-architecture container image published valid AMD64 and ARM64 manifests.
- A production upgrade preserved existing configuration, SQLite data, pairing, and registered receivers.
- Local and public health checks passed after the production upgrade.
- Manual notifications reached both desktop and Samsung receivers.
- Automatic Bash one-shot monitoring reached both receivers and disarmed correctly.

## [0.1.0-rc.1] - 2026-07-20

First public release candidate.

### Added

- Private single-owner relay and installable PWA.
- Manual terminal notifications.
- Opt-in automatic monitoring for Bash, Zsh, and Fish.
- Linux x86-64, Linux ARM64, Raspberry Pi OS, and WSL 2 support.
- Per-user daemon with bounded Unix socket IPC.
- Durable local retry queue and idempotent relay delivery.
- Tagged Web Push receivers.
- High-urgency Web Push delivery with a short notification TTL.
- Docker deployment with persistent SQLite storage.
- Security, privacy, protocol, architecture, configuration, and operations documentation.
- Open-source contribution guides and issue templates.
- Reproducible Linux CLI archives, checksums, GitHub releases, and multi-architecture container publishing.

### Changed

- Simplified public documentation for first-time users.
- Reframed internal milestone notes as a public roadmap and operations guide.

[Unreleased]: https://github.com/KingHacker9000/shellbell/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/KingHacker9000/shellbell/releases/tag/v0.1.0
[0.1.0-rc.1]: https://github.com/KingHacker9000/shellbell/releases/tag/v0.1.0-rc.1
