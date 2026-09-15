# Claude Code and Codex hooks

Shellbell works well as the notification transport for local coding agents. Prefer
agent lifecycle hooks over MCP when the only goal is to notify a person that an
agent is waiting. The hook process runs outside model context, so routine
notifications add no model tool schema and no recurring prompt tokens.

The pattern is:

```text
Claude Code / Codex -> local lifecycle hook -> shellbell ring -> relay -> phone
```

The machine must already be installed and paired with Shellbell. Verify first:

```sh
shellbell doctor
shellbell ring "Agent hook test" --to phone
```

## Shared helper

Install one small helper that any agent can call:

```sh
mkdir -p ~/.local/bin
cat > ~/.local/bin/agentbell <<'EOF'
#!/usr/bin/env bash
set -u
reason="${*:-Agent needs attention}"
host="$(hostname -s 2>/dev/null || hostname)"
project="$(basename "$PWD")"
"$HOME/.local/bin/shellbell" ring "${reason} — ${project}@${host}" --to phone \
  >/dev/null 2>&1 || true
EOF
chmod +x ~/.local/bin/agentbell
```

Test it:

```sh
~/.local/bin/agentbell "Agent needs attention"
```

The helper intentionally fails open. A notification outage must not break an
agent run.

## Claude Code

Claude Code `Notification` hooks are a good default because they are intended for
side effects such as forwarding attention notifications to another service.
User-level hooks live in `~/.claude/settings.json`.

Merge this with existing settings rather than overwriting other configuration:

```json
{
  "hooks": {
    "Notification": [
      {
        "matcher": "permission_prompt|idle_prompt|agent_needs_input|elicitation_dialog|elicitation_url_dialog",
        "hooks": [
          {
            "type": "command",
            "command": "$HOME/.local/bin/agentbell 'Claude needs attention'"
          }
        ]
      }
    ]
  }
}
```

Useful notification types include:

- `permission_prompt`: Claude has been waiting for a permission decision;
- `idle_prompt`: Claude finished and the terminal has remained unattended;
- `agent_needs_input`: an agent/team workflow needs human input;
- `elicitation_dialog` and `elicitation_url_dialog`: a tool needs an explicit
  user interaction.

Claude intentionally delays some terminal notifications until the user appears
to be away. This avoids ringing the phone while the user is already typing in
the terminal. Run `/hooks` in Claude Code to inspect the active hook.

Reference: https://code.claude.com/docs/en/hooks

## Codex

Codex discovers user hooks at `~/.codex/hooks.json`. `PermissionRequest` fires
when Codex is about to ask for an approval, which is the highest-value automatic
phone notification.

```json
{
  "description": "Shellbell mobile notifications",
  "hooks": {
    "PermissionRequest": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "$HOME/.local/bin/agentbell 'Codex needs approval'",
            "timeout": 3
          }
        ]
      }
    ]
  }
}
```

Codex requires local hooks to be reviewed and trusted. Run `/hooks` in the Codex
CLI after adding or changing the definition.

Reference: https://developers.openai.com/codex/hooks

## Optional semantic fallback

Lifecycle hooks cover permissions and normal completion/waiting states without
model tokens. A coding agent can still ask a free-form question that does not map
to a hook. If that matters for a workflow, add only one short global instruction
rather than an MCP server or a long Shellbell skill.

For Codex `~/.codex/AGENTS.md` or a repository `AGENTS.md`:

```text
When blocked waiting for user input or a user decision, run `agentbell "Codex needs input: <short reason>"` once before waiting. Do not ring for routine updates.
```

Use an equivalent one-line instruction for another agent only when its native
hooks do not already cover the desired waiting state. This keeps Shellbell a
generic notification transport and keeps token overhead close to zero.
