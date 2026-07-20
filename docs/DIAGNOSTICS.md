# Safe diagnostics

Shellbell diagnostics are designed not to expose credentials, Push subscription endpoints, encryption keys, command contents, or terminal output.

## Source-machine report

From a repository checkout:

```sh
bash scripts/diagnose.sh
```

The report includes:

- operating system and architecture;
- CLI version;
- safe effective configuration;
- relay and source-token connectivity;
- daemon IPC and durable-queue health;
- shell integration state;
- current session state when available.

Send a diagnostic notification and recheck the queue:

```sh
bash scripts/diagnose.sh --send --to all
```

A zero-pending durable queue proves that the local daemon handed the event to the relay. Web Push acceptance cannot prove that a particular device displayed the notification, so confirm receipt on the intended receiver.

## Installed-release checks

Without a repository checkout, use:

```sh
shellbell config
shellbell doctor
shellbell ring "Shellbell diagnostic test" --to all
```

## History privacy audit

Run the generic full-history audit before major public releases:

```sh
bash scripts/audit-history.sh
```

For private terms that must not be committed to the repository, create a local file containing one extended regular expression per line:

```text
private-hostname
private-domain\.example
203\.0\.113\.10
```

Then run:

```sh
SHELLBELL_PRIVATE_PATTERNS_FILE=/path/to/private-patterns.txt \
  bash scripts/audit-history.sh
```

Older public history can contain reviewed non-secret deployment references that the maintainer chooses not to rewrite after a release. Record those decisions in a separate local allowlist containing regular expressions that match the complete `revision:path:line:content` output line:

```sh
SHELLBELL_PRIVATE_PATTERNS_FILE=/path/to/private-patterns.txt \
SHELLBELL_HISTORY_ALLOWLIST_FILE=/path/to/reviewed-history.txt \
  bash scripts/audit-history.sh
```

A pass with an allowlist is reported as **passed with explicit reviewed exceptions**. It does not mean the underlying history was rewritten or that those strings were never public.

Do not commit private pattern files, history allowlists, or audit output.
