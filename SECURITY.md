# Security policy

## Supported versions

Until the first stable release, security fixes are made on the `main` branch. After releases begin, the latest published release and `main` will be supported.

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability.

Use GitHub's private vulnerability reporting from the repository **Security** tab when available. If that option is unavailable, contact the maintainer privately through GitHub before sharing technical details publicly.

Please include:

- the affected component and version or commit;
- clear reproduction steps;
- the security or privacy impact;
- whether credentials or user data may be exposed; and
- any suggested mitigation.

Do not include real credentials, Push subscription endpoints, private infrastructure details, or another person's data in the report.

## Scope

Security-sensitive areas include:

- owner and source authentication;
- browser Push subscriptions and VAPID handling;
- shell hooks and local IPC;
- local and relay SQLite data;
- durable retry behavior;
- installer and uninstall behavior;
- container and reverse-proxy deployment; and
- accidental collection of command or terminal content.

For the design-level threat model and privacy boundary, see [docs/SECURITY.md](docs/SECURITY.md).
