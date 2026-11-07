"""H8: the real-model soak's route, prompts, progress and mode checks (no model, no GPU)."""
import unittest

import soak_real as real
from soak_audit import AuditError, RunAudit
from soak_chaos import restart_times


class Route(unittest.TestCase):
    def test_the_route_points_the_local_name_at_the_model_endpoint(self):
        text = real.route("http://vllm:8000/v1", "Qwen3.8-27B-NVFP4", 32768)
        self.assertIn("name='local'", text)
        self.assertIn("protocol='openai_chat'", text)
        self.assertIn("base_url='http://vllm:8000/v1'", text)
        self.assertIn("model='Qwen3.8-27B-NVFP4'", text)
        self.assertIn("context_window=32768", text)

    def test_unsafe_endpoints_and_names_are_refused(self):
        for url in ("ftp://vllm/v1", "http://user:secret@vllm/v1", "http://vllm/v1'\nbase_url='x"):
            with self.subTest(url), self.assertRaises(AuditError):
                real.route(url, "model", 32768)
        for name in ("", "a'b", "a\nb"):
            with self.subTest(name), self.assertRaises(AuditError):
                real.route("http://vllm:8000/v1", name, 32768)
        with self.assertRaises(AuditError):
            real.route("http://vllm:8000/v1", "model", 1000)


class Prompts(unittest.TestCase):
    def test_every_message_is_the_same_small_task_with_no_approval_step(self):
        first, later = real.prompt(0), real.prompt(3)
        self.assertNotIn("approval", first + later)
        for text in (first, later):
            self.assertIn("shell", text)
            self.assertIn("read_file", text)


class Progress(unittest.TestCase):
    def test_completed_responses_are_counted_without_script_markers(self):
        progress = real.RealProgress()
        for event in ({"type": "text.delta", "text": "anything"}, {"type": "model.completed"},
                      {"type": "model.completed"}):
            progress.record(event)
        self.assertEqual(progress.completed, 2)


class Mode(unittest.TestCase):
    def test_the_real_model_run_is_4_hours_at_4_conversations_with_reconnect_faults_only(self):
        real.check_mode("real", hours=4, rehearsal=False, conversations=4)
        real.check_mode("real", hours=2, rehearsal=True, conversations=4)
        for hours, conversations in ((24, 4), (4, 8)):
            with self.subTest(hours=hours, conversations=conversations), self.assertRaises(AuditError):
                real.check_mode("real", hours=hours, rehearsal=False, conversations=conversations)
        with self.assertRaises(AuditError):
            real.check_mode("fake", hours=4, rehearsal=False, conversations=8)

    def test_the_4_hour_run_restarts_the_gateway_every_hour(self):
        self.assertEqual(restart_times(4 * 3600, False), [3600, 7200, 10800, 14400])


class Audit(unittest.TestCase):
    def test_a_real_model_may_use_any_registered_tool(self):
        audit = RunAudit(scripted_tools=False)
        audit.record({"type": "event", "seq": 1, "run_id": "r", "ts": "2000-01-01T00:00:10Z",
                      "event": {"type": "tool.call", "call_id": "c", "name": "list_dir"}})
        with self.assertRaises(AuditError):
            RunAudit().record({"type": "event", "seq": 1, "run_id": "r", "ts": "2000-01-01T00:00:10Z",
                               "event": {"type": "tool.call", "call_id": "c", "name": "list_dir"}})


if __name__ == "__main__":
    unittest.main()
