# Configuration

Shellbell stores user settings in:

```text
~/.config/shellbell/config.toml
```

Paired source credentials are stored separately in:

```text
~/.config/shellbell/config.json
```

`shellbell config` prints safe effective settings and file paths. It never prints the source token.

## Default settings

```toml
[activity]
minimum_active = "2m"
idle_for = "45s"

[delivery]
targets = ["phone"]

[display]
automatic_message = "Shell is ready"

[daemon]
queue_limit = 100
event_max_age = "24h"
```

## Activity

- `minimum_active` is the accumulated foreground command time required before a burst can notify.
- `idle_for` is how long the prompt must remain ready without another command beginning.

Command-line values such as `--after` and `--idle` apply only to the current shell session and do not rewrite the file.

## Delivery targets

Supported tags are:

```text
phone
pc
mobile
desktop
```

An empty target list means all enabled receivers. A receiver is selected when it contains at least one requested tag.

Examples:

```sh
shellbell once --to phone
shellbell on --to pc
shellbell ring "Ready" --to all
```

## Display

`automatic_message` is used for automatic notifications. Session labels set with `shellbell name` make notifications easier to identify.

## Daemon queue

- `queue_limit` bounds the number of locally queued events.
- `event_max_age` removes notifications that are too old to remain useful.

The daemon retries temporary network and server failures with bounded exponential backoff while preserving the original event ID.

## Permissions

Shellbell creates its configuration, credential, state, hook, and runtime files with owner-only permissions. Do not copy real credentials into issues, logs, or example files.
