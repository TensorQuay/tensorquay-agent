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

    @staticmethod
    def tool_error(audit, content):
        """A read_file call and its failed result, as the 17 Nov 2026 real-model run saw them (Qwen passed cursor "1")."""
        audit.record({"type": "event", "seq": 1, "run_id": "r", "ts": "2000-01-01T00:00:10Z",
                      "event": {"type": "tool.call", "call_id": "c", "name": "read_file"}})
        audit.record({"type": "event", "seq": 2, "run_id": "r", "ts": "2000-01-01T00:00:10Z",
                      "event": {"type": "tool.start", "call_id": "c"}})
        audit.record({"type": "event", "seq": 3, "run_id": "r", "ts": "2000-01-01T00:00:11Z",
                      "event": {"type": "tool.result", "call_id": "c", "status": "error", "content": content}})

    def test_a_real_models_mistaken_arguments_are_counted_not_fatal(self):
        audit = RunAudit(scripted_tools=False)
        self.tool_error(audit, "invalid_args: /cursor: not a cursor from an earlier read_file; to read by lines, leave "
                               "cursor out")
        self.assertEqual(audit.tool_outcomes, {"read_file.model_error": 1})
        self.assertEqual(audit.model_errors, [{"call_id": "c", "seq": 3, "ts": "2000-01-01T00:00:11Z",
                                               "tool": "read_file", "code": "invalid_args"}])

    def test_every_code_a_models_own_call_causes_is_a_model_error(self):
        for code in ("invalid_args", "path_invalid", "path_outside_workspace", "not_found", "unsupported",
                     "output_too_large", "denied", "egress_denied"):
            audit = RunAudit(scripted_tools=False)
            self.tool_error(audit, code + ": detail")
            self.assertEqual(audit.tool_outcomes, {"read_file.model_error": 1}, code)

    def test_a_real_model_still_fails_the_soak_on_a_system_error(self):
        for content in ("io: Input/output error", "timeout: tool deadline elapsed", "workspace_full: no room",
                        "mcp_error: down", "invalid_args"):
            with self.assertRaises(AuditError, msg=content):
                self.tool_error(RunAudit(scripted_tools=False), content)

    @staticmethod
    def shell_timeout(audit, timeout_ms, seconds):
        """A shell call and its timed-out result; the 17 Nov run's model asked for 10 s on "sleep 20"."""
        arguments = {"command": "printf soak-12 > soak.txt; sleep 20; cat soak.txt", "cwd": "/workspace"}
        if timeout_ms is not None:
            arguments["timeout_ms"] = timeout_ms
        audit.record({"type": "event", "seq": 1, "run_id": "r", "ts": "2000-01-01T00:00:10Z",
                      "event": {"type": "tool.call", "call_id": "t", "name": "shell", "arguments": arguments}})
        audit.record({"type": "event", "seq": 2, "run_id": "r", "ts": "2000-01-01T00:00:10Z",
                      "event": {"type": "tool.start", "call_id": "t"}})
        audit.record({"type": "event", "seq": 3, "run_id": "r", "ts": "2000-01-01T00:00:{:02d}Z".format(10 + seconds),
                      "event": {"type": "tool.result", "call_id": "t", "status": "error",
                                "content": "timeout: tool deadline elapsed"}})

    def test_a_deadline_the_model_set_itself_is_its_own_mistake(self):
        audit = RunAudit(scripted_tools=False)
        self.shell_timeout(audit, 10000, 10)
        self.assertEqual(audit.tool_outcomes, {"shell.model_error": 1})
        self.assertEqual(audit.model_errors[0]["code"], "timeout_requested")

    def test_a_timeout_under_the_default_deadline_or_far_from_the_asked_one_still_fails(self):
        for timeout_ms, seconds in ((None, 10), (10000, 40), (60000, 10)):
            with self.assertRaises(AuditError, msg=str((timeout_ms, seconds))):
                self.shell_timeout(RunAudit(scripted_tools=False), timeout_ms, seconds)

    def test_the_scripted_model_still_fails_on_any_tool_error(self):
        with self.assertRaises(AuditError):
            self.tool_error(RunAudit(), "invalid_args: /cursor: not a cursor from an earlier read_file")


if __name__ == "__main__":
    unittest.main()
