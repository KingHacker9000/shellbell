# Contributing to Shellbell

Thanks for helping improve Shellbell.

## Before you start

- Search existing issues and pull requests.
- Open an issue before large changes so the approach can be discussed.
- Keep pull requests focused and small enough to review.
- Never include real tokens, Push endpoints, private keys, server addresses, personal paths, or production data.

## Development setup

Required tools:

- Rust 1.97
- Node.js 24 and npm
- Docker Compose v2
- Bash
- Zsh, Fish, and util-linux for the full shell integration suite

Run the main checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets --locked

npm --prefix pwa ci
npm --prefix pwa run typecheck
npm --prefix pwa run lint
npm --prefix pwa test
npm --prefix pwa run build
npm --prefix pwa run validate:manifest
```

See [Local development](docs/LOCAL_DEVELOPMENT.md) for manual verification and Docker checks.

## Privacy rules

Shellbell's privacy boundary is part of its public contract.

A contribution must not collect or transmit:

- command text;
- terminal output;
- exit codes or inferred success and failure;
- environment variables;
- working directories;
- shell history;
- process lists; or
- unrelated user activity.

Changes to IPC, persistence, logging, notification payloads, authentication, or shell hooks require explicit privacy and security review.

## Pull requests

Include:

- a clear summary;
- why the change is needed;
- tests performed;
- any compatibility or migration impact; and
- confirmation that no private or identifying information was added.

By contributing, you agree that your contribution is licensed under the project's MIT License.
