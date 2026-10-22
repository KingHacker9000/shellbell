import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("collector", Path(__file__).with_name("run-collector.py"))
collector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(collector)


class CollectorTests(unittest.TestCase):
    def test_hook_allowlist_and_question_states(self):
        with tempfile.TemporaryDirectory() as directory:
            os.environ["SHELLBELL_STATE_DIR"] = directory
            for provider, payload, state in [
                ("claude", {"hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion"}, "waiting_input"),
                ("codex", {"hook_event_name": "PreToolUse", "tool_name": "request_user_input"}, "waiting_input"),
                ("codex", {"hook_event_name": "PermissionRequest"}, "waiting_approval"),
                ("codex", {"type": "agent-turn-complete", "turn-id": "turn"}, "finished"),
            ]:
                collector.hook(provider, {**payload, "session_id": "session", "prompt": "SECRET", "cwd": "SECRET", "tool_input": {"question": "SECRET"}, "last-assistant-message": "SECRET"})
                data = list(Path(directory, "beacons").glob("*.json"))
                value = json.loads(data[-1].read_text()) if len(data) == 1 else next(json.loads(p.read_text()) for p in data if json.loads(p.read_text())["beacon"]["state"] == state)
                self.assertEqual(value["beacon"]["state"], state)
                self.assertNotIn("SECRET", "".join(p.read_text() for p in data))
            os.environ.pop("SHELLBELL_STATE_DIR")

    def test_scheduler_disappearance_is_not_success(self):
        self.assertEqual(collector.parse_slurm("12|RUNNING\n13_4|FAILED\n14.batch|COMPLETED\n15|CANCELLED by 123\n"), {"12": "running", "13_4": "failed", "15": "cancelled"})
        self.assertEqual(collector.slurm_state("NOT_RECOGNIZED"), "unknown")

    def test_job_display_details_are_selected_and_do_not_request_commands(self):
        fields = ["123_4", "RUNNING", "training-baseline", "gpu", "12:31", "2:00:00", "4", "48G", "gres/gpu:example:1", "2026-10-17T12:00:00", "node1"]
        states, details = collector.parse_queue("\x1f".join(fields) + "\n")
        self.assertEqual(states, {"123_4": "running"})
        self.assertEqual(details["123_4"]["name"], "training-baseline")
        self.assertEqual(details["123_4"]["cpus"], 4)
        self.assertEqual(set(details["123_4"]), {"name", "partition", "elapsed", "time_limit", "cpus", "min_memory", "tres_per_node", "start_time", "node_reason"})
        self.assertEqual(collector.parse_queue("malformed output"), ({}, {}))
        history = ["123_4", "COMPLETED", "training-baseline", "gpu", "19:10", "2:00:00", "4", "48Gn", "2026-10-17T12:00:00", "node1"]
        states, details = collector.parse_accounting_details("\x1f".join(history))
        self.assertEqual(states["123_4"], "finished")
        self.assertEqual(details["123_4"]["name"], "training-baseline")
        self.assertEqual(details["123_4"]["tres_per_node"], "")


if __name__ == "__main__":
    unittest.main()
