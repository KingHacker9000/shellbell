# Local development and verification

Prerequisites are Rust 1.97, Node.js 24/npm, Docker Compose v2, and Bash. Install Zsh and Fish for the complete shell suite. All installer tests use temporary homes; never point `--home` at the developer home during automated experiments.

## Full native validation

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

ARM64 compile check:

```sh
rustup target add aarch64-unknown-linux-gnu
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
cargo check --locked --target aarch64-unknown-linux-gnu \
  -p shellbell-cli -p shellbell-local -p shellbell-protocol
```

Docker validation without a real secret:

```sh
SHELLBELL_OWNER_BOOTSTRAP_TOKEN=placeholder-owner-token-at-least-32-bytes \
SHELLBELL_VAPID_PUBLIC_KEY=placeholder-public \
SHELLBELL_VAPID_PRIVATE_KEY=placeholder-private \
docker compose -f deploy/docker-compose.yml config --quiet
docker build -f deploy/Dockerfile -t shellbell:milestone-2 .
```

For relay/PWA development, configure VAPID values, run `cargo run -p shellbell-relay`, and run `npm --prefix pwa run dev`. Vite proxies `/api` and `/health` to port 8080. Pairing credentials stay in the client's existing config JSON.

## Isolated installer check

```sh
TEST_HOME="$(mktemp -d)"
cargo run -p shellbell-cli -- install --all-shells --home "$TEST_HOME"
find "$TEST_HOME" -maxdepth 5 -type f -print
cargo run -p shellbell-cli -- uninstall --home "$TEST_HOME"
```

The shell integration test executable builds a fake local hook endpoint that logs only session UUID plus event arguments. It injects a random private token into an executed command and asserts that the token never appears in event logs.

## Exact manual verification

These checks assume the source is paired and a receiver is active. In every case, inspect `shellbell status` before/after and expect calm wording, never success/failure wording.

### 1. Current WSL Bash

```sh
cargo build --release -p shellbell-cli
export PATH="$PWD/target/release:$PATH"
shellbell install --shell bash
exec bash
shellbell once --after 10s --idle 5s
sleep 15
# Wait 5 seconds at the returned prompt: exactly one ring.
```

Burst accumulation/cancel test:

```sh
shellbell on --after 10s --idle 10s
sleep 12
# Enter the next command within 10 seconds of prompt return.
sleep 12
# Wait 10 seconds: one ring only after the final idle window.
shellbell off
```

### 2. Linux Bash

```sh
shellbell install --shell bash
exec bash
shellbell once --after 10s --idle 5s
sleep 15
shellbell status
```

Also run the two-command accumulation test above.

### 3. Zsh

```sh
shellbell install --shell zsh
exec zsh
shellbell once --after 10s --idle 5s
sleep 15
shellbell status
```

Confirm Oh My Zsh/Starship prompt appearance and existing hooks are unchanged.

### 4. Fish

```fish
shellbell install --shell fish
exec fish
shellbell once --after 10s --idle 5s
sleep 15
shellbell status
```

Confirm the existing `fish_prompt` is unchanged.

### 5. Raspberry Pi ARM64

On a 64-bit Raspberry Pi OS host:

```sh
uname -m
# Expected: aarch64
cargo build --release -p shellbell-cli
export PATH="$PWD/target/release:$PATH"
shellbell install --shell bash
exec bash
shellbell once --after 10s --idle 5s
sleep 15
```

### 6. Remote DGP Linux shell

After copying/building the client on the remote host and pairing that host if it has no source credential:

```sh
ssh user@dgp-host
shellbell install --shell bash
exec bash
shellbell name "DGP remote"
shellbell once --after 10s --idle 5s --to phone
sleep 15
```

Disconnect/reconnect and verify `shellbell doctor`; no command content should appear at the relay.

### 7. tmux panes

```sh
tmux new-session -s shellbell-test
# Pane 1
shellbell name "tmux pane 1"
shellbell once --after 10s --idle 5s
sleep 15
# Split with Ctrl-b %, then in pane 2:
shellbell status
shellbell name "tmux pane 2"
shellbell once --after 10s --idle 5s
sleep 15
```

The IDs and labels must differ and each pane rings once.

### 8. systemd-enabled environment

```sh
systemctl --user show-environment >/dev/null
shellbell install --shell bash
systemctl --user is-enabled shellbell.service
systemctl --user is-active shellbell.service
shellbell doctor
shellbell once --after 10s --idle 5s
sleep 15
```

Restart the daemon during settling to verify recovery:

```sh
shellbell once --after 10s --idle 15s
sleep 12
systemctl --user restart shellbell.service
# Wait for the original settling deadline: one ring, no duplicate.
```

### 9. no-systemd fallback environment

In a container/WSL distro without a user manager:

```sh
! systemctl --user show-environment
shellbell install --shell bash
exec bash
shellbell doctor
shellbell once --after 10s --idle 5s
sleep 15
```

Install output must name shell-start lazy activation, the first shell must create the private socket, and `/etc/wsl.conf` must remain unchanged.

## Boundary checks

Background limitation:

```sh
shellbell once --after 10s --idle 5s
sleep 15 &
# No notification should be attributed to the 15-second background lifetime.
shellbell off
```

Interactive-program limitation and manual escape hatch:

```sh
shellbell once --after 10s --idle 5s
python3
# Leave the REPL open: no automatic ring because the shell prompt has not returned.
# From an integrated child/application when appropriate:
shellbell ring "Task complete"
```

Typing-at-prompt limitation: use a five-second idle value, return to the prompt, begin typing without pressing Enter, and observe that the deadline may still ring.
