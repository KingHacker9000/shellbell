# Releasing Shellbell

Releases are created from signed-off commits on `main` by pushing a version tag.

## Before tagging

Confirm that:

- the working tree is clean;
- `main` matches `origin/main`;
- required CI checks are green;
- `CHANGELOG.md` contains the release section;
- Cargo and PWA package versions match the tag's base version;
- the current production data directory and deployment files are backed up.

Run:

```sh
bash scripts/check-release-version.sh v0.1.0-rc.1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets --locked
npm --prefix pwa ci
npm --prefix pwa run typecheck
npm --prefix pwa run lint
npm --prefix pwa test
npm --prefix pwa run build
bash scripts/check-public-content.sh
```

## Create a release candidate

```sh
git switch main
git pull --ff-only origin main
git tag -a v0.1.0-rc.1 -m "Shellbell v0.1.0-rc.1"
git push origin v0.1.0-rc.1
```

The release workflow publishes:

- Linux x86-64 and ARM64 CLI archives;
- `SHA256SUMS`;
- a GitHub prerelease;
- a multi-architecture container image tagged with the release tag.

A tag containing a hyphen is published as a prerelease. A stable semantic-version tag is marked as the latest release.

## Verify the release

Download the release assets and verify:

```sh
sha256sum -c SHA256SUMS
```

Extract the archive for the current platform and run:

```sh
./shellbell --version
```

Verify the container image and health endpoint in a non-production deployment before upgrading production.

## Production upgrade

Use the immutable release tag, preserve the existing `.env` and data directory, and follow [Operations](OPERATIONS.md). Keep the previous image available until pairing, manual notifications, automatic monitoring, restart recovery, PC delivery, and phone delivery have passed.

## Stable release

After the release candidate has passed production testing:

1. update `CHANGELOG.md` for `v0.1.0`;
2. merge the release PR;
3. create and push the annotated `v0.1.0` tag;
4. verify the published archives, checksums, container image, and GitHub release;
5. redeploy using the stable immutable tag.
