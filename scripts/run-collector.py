#!/usr/bin/env python3
"""Metadata-only producers; hooks never access relay credentials or the network."""
import argparse
import datetime as dt
import json
import os
from pathlib import Path
import re
import socket
import shlex
import subprocess
import sys
import tempfile
import uuid

STATES = {"started", "running", "queued", "waiting_input", "waiting_approval", "finished", "failed", "cancelled", "ended", "unknown", "needs_reauth"}
IDENT = re.compile(r"^[A-Za-z0-9._:@+\-]{1,128}$")


class SchedulerError(RuntimeError):
    pass


def state_dir():
    return Path(os.environ.get("SHELLBELL_STATE_DIR", Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share")) / "shellbell"))


def emit(collector, run_id, state, machine=None, heartbeat=False, event_id=None, stale=3600, job=None):
    machine = machine or socket.gethostname().split(".")[0]
    if state not in STATES or not all(IDENT.fullmatch(str(v)) for v in (collector, run_id, machine)):
        raise ValueError("invalid beacon metadata")
    event_id = str(event_id or uuid.uuid4())
    # Construct an allowlist, never copy an upstream dictionary.
    value = {"event_id": event_id, "target_tags": [], "message": None, "beacon": {
        "collector": collector, "machine": machine, "run_id": str(run_id), "state": state,
        "observed_at": dt.datetime.now(dt.timezone.utc).isoformat(), "heartbeat": heartbeat,
        "stale_after_seconds": stale,
    }}
    if job is not None:
        value["beacon"]["job"] = job
    spool = state_dir() / "beacons"
    spool.mkdir(mode=0o700, parents=True, exist_ok=True)
    if sum(1 for _ in spool.glob("*.json")) >= 5000:
        raise RuntimeError("beacon spool full")
    fd, tmp = tempfile.mkstemp(prefix=".beacon-", dir=spool)
    try:
        with os.fdopen(fd, "w") as out:
            json.dump(value, out, separators=(",", ":"))
            out.flush()
            os.fsync(out.fileno())
        os.replace(tmp, spool / (event_id + ".json"))
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)
    return value


def hook(provider, payload):
    event = payload.get("hook_event_name", "")
    tool = str(payload.get("tool_name", "")).rsplit(".", 1)[-1]
    run_id = payload.get("session_id") or payload.get("thread-id")
    if not run_id:
        return
    state = None
    if payload.get("type") == "agent-turn-complete":
        state = "finished"
    elif event == "SessionStart":
        state = "started"
    elif event == "UserPromptSubmit":
        state = "running"
    elif event == "PermissionRequest":
        state = "waiting_approval"
    elif event == "PreToolUse" and tool in {"AskUserQuestion", "request_user_input", "request_user_input_async"}:
        state = "waiting_input"
    elif event == "PostToolUse" and tool in {"AskUserQuestion", "request_user_input", "request_user_input_async"}:
        state = "running"
    elif event == "Notification":
        kind = payload.get("notification_type", "")
        if kind == "permission_prompt":
            state = "waiting_approval"
        elif kind == "idle_prompt":
            state = "finished"
        elif kind in {"agent_needs_input", "elicitation_dialog", "elicitation_url_dialog"}:
            state = "waiting_input"
    elif event == "Stop":
        state = "finished"
    elif event == "SessionEnd":
        state = "ended"
    if state:
        key = None
        if payload.get("type") == "agent-turn-complete" and payload.get("turn-id"):
            key = uuid.uuid5(uuid.NAMESPACE_URL, "shellbell:codex:" + str(run_id) + ":" + str(payload["turn-id"]))
        emit(provider, run_id, state, event_id=key)


def slurm_state(raw):
    value = raw.split()[0].rstrip("+")
    if value in {"PENDING", "REQUEUE_FED", "REQUEUED", "RESV_DEL_HOLD"}:
        return "queued"
    if value in {"RUNNING", "COMPLETING", "CONFIGURING", "SUSPENDED", "RESIZING", "STAGE_OUT"}:
        return "running"
    if value == "COMPLETED":
        return "finished"
    if value == "CANCELLED":
        return "cancelled"
    if value in {"FAILED", "TIMEOUT", "NODE_FAIL", "OUT_OF_MEMORY", "BOOT_FAIL", "DEADLINE", "PREEMPTED", "REVOKED"}:
        return "failed"
    return "unknown"


def parse_slurm(text):
    result = {}
    for line in text.splitlines():
        fields = line.strip().split("|")
        if len(fields) >= 2 and re.fullmatch(r"[0-9]+(?:[_+][0-9]+)?", fields[0]) and fields[1]:
            result[fields[0]] = slurm_state(fields[1])
    return result


def parse_queue(text):
    """Only the explicitly requested display fields, separated without truncation."""
    result, details = {}, {}
    for line in text.split("\n"):
        fields = line.strip().split("\x1f")
        if len(fields) != 11 or not re.fullmatch(r"[0-9]+(?:[_+][0-9]+)?", fields[0]):
            continue
        result[fields[0]] = slurm_state(fields[1])
        clean = lambda value: "".join(c for c in value.strip() if c.isprintable())[:256]
        details[fields[0]] = dict(zip(["name", "partition", "elapsed", "time_limit", "min_memory", "tres_per_node", "start_time", "node_reason"], map(clean, fields[2:6] + fields[7:11])))
        details[fields[0]]["cpus"] = int(fields[6]) if fields[6].isdigit() and int(fields[6]) <= 1_000_000 else None
    return result, details


def atomic_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd, tmp = tempfile.mkstemp(dir=path.parent)
    with os.fdopen(fd, "w") as out:
        json.dump(value, out)
        out.flush()
        os.fsync(out.fileno())
    os.replace(tmp, path)


def poll(config_path):
    config = json.loads(Path(config_path).read_text())
    path = state_dir() / "slurm-state.json"
    previous = json.loads(path.read_text()) if path.exists() else {}
    details_path = state_dir() / "slurm-details.json"
    details_cache = json.loads(details_path.read_text()) if details_path.exists() else {}
    for target in config.get("clusters", []):
        if not target.get("enabled", False):
            continue
        host = target["ssh_host"]
        machine = target.get("machine", host)
        if not IDENT.fullmatch(host) or not IDENT.fullmatch(machine):
            raise ValueError("invalid SSH alias")
        prefix = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ConnectionAttempts=1", "-o", "StrictHostKeyChecking=yes"]
        if config.get("ssh_config"):
            prefix += ["-F", config["ssh_config"]]
        before = previous.get(machine, {})
        binary_dir = target.get("slurm_bin", "")
        if binary_dir and not re.fullmatch(r"/[A-Za-z0-9/._-]+", binary_dir):
            raise ValueError("invalid scheduler binary directory")
        squeue = shlex.quote(binary_dir.rstrip("/") + "/squeue") if binary_dir else "squeue"
        sacct = shlex.quote(binary_dir.rstrip("/") + "/sacct") if binary_dir else "sacct"
        def remote(command):
            return "bash -lc " + shlex.quote(command) if target.get("login_shell") else command
        try:
            # Job display details are a separate opt-in. Never request command/path/output fields.
            detailed = target.get("job_details", False)
            display_format = "\x1f".join(["%i", "%T", "%j", "%P", "%M", "%l", "%C", "%m", "%b", "%S", "%R"]) if detailed else "%i|%T"
            queue = subprocess.run(prefix + [host, remote(squeue + " --me --noheader --array --format=" + shlex.quote(display_format))], capture_output=True, text=True, timeout=25)
            if queue.returncode:
                if queue.returncode == 255:
                    raise ConnectionError("SSH unavailable")
                raise SchedulerError("scheduler unavailable")
            if detailed:
                current, display_details = parse_queue(queue.stdout)
                # A malformed/nonstandard queue response cannot be treated as an empty queue.
                if queue.stdout.strip() and not current:
                    raise SchedulerError("scheduler format unsupported")
            else:
                current, display_details = parse_slurm(queue.stdout), {}
            now = dt.datetime.now(dt.timezone.utc).timestamp()
            cached = {key: value for key, value in details_cache.get(machine, {}).items() if now - value.get("seen_at", 0) < 2 * 86400}
            for key, value in display_details.items():
                cached[key] = {"job": value, "seen_at": now}
            details_cache[machine] = dict(list(cached.items())[-2000:]) if detailed else {}
            missing = {job: state for job, state in before.items() if state in {"running", "queued", "unknown"} and job not in current}
            if missing:
                accounting = subprocess.run(prefix + [host, remote(sacct + " --noheader --parsable2 --allocations --starttime=now-2days --format=JobIDRaw,State")], capture_output=True, text=True, timeout=25)
                history = parse_slurm(accounting.stdout) if accounting.returncode == 0 else {}
                for job in missing:
                    current[job] = history.get(job, "unknown")
            # Preserve terminal identities without issuing duplicate pings on each poll.
            current = {**{k: v for k, v in before.items() if v in {"finished", "failed", "cancelled"}}, **current}
            for job, state in current.items():
                if state != before.get(job) or state in {"running", "queued"}:
                    emit("slurm", job, state, machine, heartbeat=(state == before.get(job)), stale=600, job=cached.get(job, {}).get("job") if detailed else None)
            previous[machine] = dict(list(current.items())[-2000:])
            emit("slurm-collector", "connection", "running", machine, heartbeat=True, stale=600)
        except ConnectionError:
            # Never launch interactive/Duo authentication in a background timer.
            emit("slurm-collector", "connection", "needs_reauth", machine, heartbeat=before.get("_connection") == "needs_reauth", stale=600)
            previous.setdefault(machine, {})["_connection"] = "needs_reauth"
        except (SchedulerError, subprocess.TimeoutExpired):
            emit("slurm-collector", "connection", "unknown", machine, heartbeat=before.get("_connection") == "unknown", stale=600)
            previous.setdefault(machine, {})["_connection"] = "unknown"
    atomic_json(path, previous)
    atomic_json(details_path, details_cache)


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    h = sub.add_parser("hook")
    h.add_argument("provider", choices=["claude", "codex"])
    h.add_argument("payload", nargs="?")
    e = sub.add_parser("emit")
    e.add_argument("collector")
    e.add_argument("run_id")
    e.add_argument("state", choices=sorted(STATES))
    e.add_argument("--machine")
    p = sub.add_parser("poll-slurm")
    p.add_argument("config")
    args = parser.parse_args()
    if args.command == "hook":
        try:
            raw = args.payload if args.payload is not None else sys.stdin.read(1024 * 1024)
            hook(args.provider, json.loads(raw))
        except Exception:
            pass  # Fail open, emit nothing into model context, never log payloads.
    elif args.command == "emit":
        emit(args.collector, args.run_id, args.state, args.machine)
    else:
        poll(args.config)


if __name__ == "__main__":
    main()
