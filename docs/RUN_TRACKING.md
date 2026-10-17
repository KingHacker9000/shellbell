# Modular run tracking

Run tracking is an explicit, opt-in metadata extension. Existing automatic shell
monitoring still observes only activity boundaries and does not infer outcomes.
Collectors report their own lifecycle or scheduler states. No command, prompt,
question text, response, tool input, directory, exit code, environment or process
list is collected. Agent identifiers are intentionally sent only when enabled.

## Data flow

```text
Claude / Codex hooks ── metadata-only atomic local spool ── local daemon
Slurm read-only poll ── metadata-only atomic local spool ── local daemon
local daemon ── existing paired source auth + durable retry ── relay
relay ── Web Push / scoped pull receiver ── phone / Totem
```

The daemon drains `beacons/*.json` under its existing state directory. Producers
never read source credentials or perform relay HTTP requests. The spool caps at
5,000 files; the existing daemon queue limit and maximum age still apply. Invalid
spool records are discarded. An offline relay cannot block an agent hook.

`scripts/run-collector.py` supports independent `hook`, `emit`, and `poll-slurm`
producers. Collector names are identifiers rather than a fixed provider enum, so
another adapter can use the same contract without relay changes.

## Install on a paired Linux/WSL source

Upgrade the daemon, then install `run-collector.py` as
`~/.local/bin/shellbell-collector`. Run `scripts/install-run-hooks.py` to merge
Claude/Codex hook definitions and add completion `notify` if none exists. Existing
hooks/settings are preserved and backed up. Restart the local Shellbell service
and open new agent sessions. Review/trust Codex hook definitions with `/hooks`;
the installer never changes hook trust or approval permissions.

Claude reports session start/end, prompt submission, Stop, permission requests,
question tools, and attention notifications. Codex `notify` reports turn completion;
supported hooks add lifecycle, permissions and `request_user_input` question-tool
events. PostToolUse on a question marks the run running again after the answer.
Native runtime hooks must be supported/enabled in the installed agent version.

Free-form prose questions cannot be reliably recognized without reading model
output. A completed turn still pings via completion. For a runtime without a
question hook, an explicit fallback can call:

```sh
shellbell-collector emit codex SESSION_ID waiting_input
```

No question text is sent. A `finished` state means an agent turn ended, not that
the user's overall task succeeded. `ended` means the session closed. Lost contact
is displayed as stale, not completed/failed. Agent hooks have a one-hour freshness
window; no background process fabricates liveness from an open session.

## Slurm

Configure a private JSON file:

```json
{"clusters":[{"ssh_host":"cluster-alias","machine":"cluster-name","enabled":true}]}
```

An optional `ssh_config` names an existing OpenSSH config. Run
`shellbell-collector poll-slurm CONFIG` on a two-minute systemd timer, from a host
that already has the necessary credentials and SSH ControlMaster sessions.
Only `squeue --me --noheader --array --format='%i|%T'` and, for missing jobs,
`sacct --noheader --parsable2 --allocations --starttime=now-2days --format=JobIDRaw,State`
run remotely. There is no job submission, cancellation, compute-node install or
Tailscale installation on a shared cluster. SSH uses BatchMode, strict host key
checking and bounded timeouts. A failed SSH/scheduler query surfaces as
`needs_reauth`; it never starts interactive Duo authentication in a timer.

An empty queue alone never proves success: vanished jobs use accounting, or
remain unknown if accounting is unavailable. Job arrays are tracked individually.
The poller's private transition cache retains up to 2,000 job identities per host;
the relay's current view and inbox expire after two days. Active jobs heartbeat
on each successful poll with a ten-minute freshness window.

## HTTP and retention

`POST /api/beacons` uses the existing source credential and acceptance response:

```json
{"event_id":"UUID","target_tags":[],"beacon":{"collector":"codex","machine":"workstation","run_id":"SESSION_ID","state":"waiting_input","observed_at":"2026-10-17T12:00:00Z","heartbeat":false,"stale_after_seconds":3600}}
```

Identifiers allow 1–128 ASCII alphanumeric or `._:@+-` characters. Unknown beacon
fields are rejected. Observation timestamps allow up to five minutes of future
clock skew and two days of history. Newer observed events win; a heartbeat cannot
change the current state. Replayed event IDs cannot duplicate history or alerts.

Owner `GET /api/runs` returns the live snapshot and two-day inbox. Owner + CSRF
`DELETE /api/run-inbox` clears the inbox;
`DELETE /api/run-inbox/SOURCE_ID/EVENT_ID` dismisses one entry. Dismissal preserves
idempotency and current status. Heartbeats do not appear in the inbox or alert
feed. Waiting for input/approval, finish, failure, cancellation and re-authentication
trigger alerts; other transitions update live status and inbox quietly.

`GET /api/receiver-runs` uses a dedicated pull receiver credential and applies
receiver tag routing. It returns no owner inbox or configuration. The existing
`/api/receiver-feed` gains optional `beacon` metadata on run alerts; legacy rings
remain compatible. Totem uses deterministic attention IDs and persists delivery
progress before acknowledging the relay cursor. It never receives owner auth.

## Rollback

Back up SQLite with its online backup API and preserve the pinned image/Compose
file before deployment. Do not open a database containing the new SQL migrations
with an older SQLx binary: roll back the database backup and image together. Local
daemon changes are additive; the old binary can ignore the new queue column.
Stop the Totem connector user service, restore the prior registry/config backup,
and let the Portal host restart to remove its app/widget.
