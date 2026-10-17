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


if __name__ == "__main__":
    unittest.main()
