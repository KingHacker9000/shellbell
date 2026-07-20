# Relay backup and restore

The helper scripts operate on a Compose deployment with persistent relay data mounted at `/data`. They never print `.env` contents, Push endpoints, private VAPID keys, or source tokens.

## Create a backup

From a repository checkout on the relay host:

```sh
sudo -v
bash scripts/relay-backup.sh
```

The default stack path is `/opt/stacks/shellbell`. Override it when needed:

```sh
bash scripts/relay-backup.sh \
  --stack /srv/shellbell \
  --output /srv/backups/shellbell-$(date +%F)
```

The script:

1. validates the Compose configuration;
2. records the current immutable image reference;
3. stops the relay only while copying SQLite data;
4. archives the complete `/data` mount;
5. copies `compose.yaml` and `.env` without printing them;
6. writes and verifies a checksum manifest;
7. restarts the service if it was previously running.

A backup directory contains:

```text
compose.yaml
.env
data.tar.gz
metadata.txt
SHA256SUMS
```

Store backups somewhere separate from the relay host.

## Restore a backup

Restoration is destructive and requires an explicit `--yes`:

```sh
sudo -v
bash scripts/relay-restore.sh \
  --backup /path/to/backup \
  --yes
```

Before changing data, the restore helper creates a new `pre-restore-*` safety backup. It then verifies checksums, restores the Compose configuration, `.env`, and `/data`, starts the relay, and waits for health. If health fails, it automatically reapplies the safety snapshot.

After restoration, verify:

```sh
curl -fsS http://127.0.0.1:8080/health
shellbell doctor
shellbell ring "Restore validation" --to all
```
