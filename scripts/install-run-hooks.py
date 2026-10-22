#!/usr/bin/env python3
"""Merge metadata-only hooks without replacing existing user settings."""
import datetime
import json
from pathlib import Path
import shutil
import re
import sys

home = Path.home()
helper = home / ".local/bin/shellbell-collector"
stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
backup = home / ".local/share/shellbell/backups" / stamp
backup.mkdir(parents=True, mode=0o700)


def merge(path, provider):
    if path.exists():
        shutil.copy2(path, backup / (provider + "-" + path.name))
        value = json.loads(path.read_text())
    else:
        value = {}
    hooks = value.setdefault("hooks", {})
    events = {"SessionStart": None, "UserPromptSubmit": None, "SessionEnd": None,
              "PermissionRequest": None,
              "PreToolUse": "(^|.*\\.)(AskUserQuestion|request_user_input|request_user_input_async)$",
              "PostToolUse": "(^|.*\\.)(AskUserQuestion|request_user_input|request_user_input_async)$"}
    if provider == "claude":
        events.update({"Stop": None, "Notification": "permission_prompt|idle_prompt|agent_needs_input|elicitation_dialog|elicitation_url_dialog"})
    for event, matcher in events.items():
        entries = hooks.setdefault(event, [])
        retained = []
        for entry in entries:
            remaining = [h for h in entry.get("hooks", []) if h.get("command", "") != f"python3 {helper} hook {provider}"]
            if remaining:
                retained.append({**entry, "hooks": remaining})
        entries[:] = retained
        entry = {"hooks": [{"type": "command", "command": f"python3 {helper} hook {provider}", "timeout": 3}]}
        if matcher:
            entry["matcher"] = matcher
        entries.append(entry)
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    path.write_text(json.dumps(value, indent=2) + "\n")
    path.chmod(0o600)


merge(home / ".codex/hooks.json", "codex")
merge(home / ".claude/settings.json", "claude")
config = home / ".codex/config.toml"
text = config.read_text() if config.exists() else ""
if config.exists():
    shutil.copy2(config, backup / "codex-config.toml")
# notify is a user-level singleton. Preserve an existing notification command.
if not re.search(r"(?m)^\s*notify\s*=", text):
    text = 'notify = ["python3", ' + json.dumps(str(helper)) + ', "hook", "codex"]\n' + text
    config.write_text(text)
    config.chmod(0o600)
else:
    print("Existing Codex notify preserved; merge shellbell completion into that command if needed")
print("Merged Claude/Codex hook definitions; backups:", backup)
print("Review/trust Codex hooks with /hooks; hook changes apply to new sessions")
