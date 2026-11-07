"""Offline boundaries for the opt-in WEB-E2E-004 stack driver."""

import io
import contextlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import compose
import demo
import demo_key_use


class DemoTests(unittest.TestCase):
    def test_runtime_pin_uses_only_a_validated_immutable_image_id(self):
        for value, valid in [("sha256:" + "a" * 64, True), ("tqa-runtime:base", False),
                             ("sha256:abc", False), ("sha256:" + "A" * 64, False)]:
            env = {"TQA_RUNTIME_IMAGE": "my-runtime:dev"}
            with patch.object(compose, "command", return_value=unittest.mock.Mock(stdout=value)):
                if valid:
                    self.assertEqual(demo.runtime_image(env), value)
                else:
                    with self.assertRaises(ValueError):
                        demo.runtime_image(env)
        with self.assertRaisesRegex(ValueError, "TQA_RUNTIME_IMAGE must name"):
            demo.runtime_image({})

    def test_demo_prepares_real_runtime_and_browser_origin(self):
        with tempfile.TemporaryDirectory() as temporary:
            workstation, runtime_id = "sha256:" + "b" * 64, "sha256:" + "c" * 64
            directory, env = Path(temporary), {"TQA_WORKSTATION_IMAGE": workstation, "TQA_GATEWAY_IMAGE": "gw",
                                               "TQA_WEB_IMAGE": "web"}
            with patch.object(compose, "command"), patch.object(compose, "ownership"), \
                    patch.object(compose, "engine_subnets", return_value=[]), \
                    patch.object(compose, "ensure_image"), patch.object(compose, "gateway_takes_pins", return_value=True), \
                    patch.object(compose, "image_id",  # a unit gate never inspects or pulls an image
                                 side_effect=lambda reference, _env: reference if reference.startswith("sha256:")
                                 else runtime_id):
                compose.prepare(directory, env, runtime="tqa-runtime:base", port=12345)
            config = (directory / "gateway.toml").read_text()
            self.assertIn('image="tqa-runtime:base"', config)
            # The built-ins are pinned to the images under test, never the engine's shared tags.
            self.assertIn('[runtime.builtin_images]\nminimal = "{}"\nworkstation = "{}"\n'.format(runtime_id, workstation),
                          config)
            self.assertEqual(env["TQA_PINNED_IMAGES"], ",".join(sorted([runtime_id, workstation])))
            self.assertIn("public_origin='http://127.0.0.1:12345'", config)
            self.assertEqual(env["TQA_HTTP_PORT"], "12345")
            self.assertEqual((directory / "install.key").stat().st_mode & 0o777, 0o600)

    def test_host_file_must_belong_to_the_reported_conversation(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            result = directory / "browser.json"
            identity = "00000000-0000-0000-0000-000000000001"
            result.write_text(json.dumps({"conversationId": identity}))
            env = {"TQA_STACK_RESULT": str(result), "TQA_STORAGE_ROOT": str(directory)}
            with self.assertRaises(AssertionError):
                demo.verify_workspace(env)
            target = directory / "workspaces" / identity / "hello" / "README.md"
            target.parent.mkdir(parents=True)
            # Only created, never changed: the demo's second request didn't reach the host.
            target.write_text("# Hello\nA small demonstration project.\n")
            with self.assertRaises(AssertionError):
                demo.verify_workspace(env)
            target.write_text(demo.DEMO_README)
            demo.verify_workspace(env)
            result.write_text(json.dumps({"conversationId": "../outside"}))
            with self.assertRaises(ValueError):
                demo.verify_workspace(env)

    def test_accounts_result_needs_both_answers_and_never_holds_a_token(self):
        with tempfile.TemporaryDirectory() as temporary:
            result = Path(temporary) / "browser.json"
            env = {"TQA_STACK_RESULT": str(result)}
            result.write_text(json.dumps({"bearer": [200, 401], "joined": True}))
            demo.verify_accounts(env)
            for value in ({"bearer": [200, 200], "joined": True}, {"bearer": [401, 401], "joined": True},
                          {"bearer": [200, 401], "joined": False}, {"bearer": [200, 401], "joined": True,
                                                                     "token": "tqa_pat_" + "a" * 43}):
                result.write_text(json.dumps(value))
                with self.assertRaises(AssertionError):
                    demo.verify_accounts(env)

    def test_channel_members_result_needs_the_lead_and_the_agent_then_ended_then_gone(self):
        with tempfile.TemporaryDirectory() as temporary:
            result = Path(temporary) / "browser.json"
            env = {"TQA_STACK_RESULT": str(result)}
            good = {"members": ["conversation:member", "user:lead"], "state": "ended", "deleted": 404}
            result.write_text(json.dumps(good))
            with contextlib.redirect_stdout(io.StringIO()):
                demo.verify_channel_members(env)
            for change in ({"members": ["conversation:member", "user:lead", "user:member"]},
                           {"members": ["user:lead"]}, {"state": "open"}, {"deleted": 200}):
                result.write_text(json.dumps({**good, **change}))
                with self.assertRaises(AssertionError):
                    demo.verify_channel_members(env)

    def test_key_use_needs_the_gateways_reason_plain_words_and_no_model_request(self):
        logs = '{"method":"POST","path":"/v1/chat/completions","turn":0,"status":200}\nnot json\n' \
               '{"method":"GET","path":"/v1/models","turn":null,"status":200}\n'
        self.assertEqual(demo_key_use.requests_in(logs), 1)
        with tempfile.TemporaryDirectory() as temporary:
            result = Path(temporary) / "browser.json"
            env = {"TQA_STACK_RESULT": str(result), "TQA_KEY_USE_BEFORE": "3", "TQA_KEY_USE_AFTER": "3"}
            good = {"code": "provider_error", "reason": "gateway.key_use_unrecorded 503", "providerMessage": None,
                    "rawShown": []}
            result.write_text(json.dumps(good))
            with contextlib.redirect_stdout(io.StringIO()):
                demo_key_use.verify_result(env)
            for change in ({"reason": "http 503"}, {"providerMessage": "upstream said no"}, {"rawShown": ["503"]},
                           {"code": None}):
                result.write_text(json.dumps({**good, **change}))
                with self.assertRaises(AssertionError):
                    demo_key_use.verify_result(env)
            # A request that reached the model while the table was broken fails the suite.
            result.write_text(json.dumps(good))
            with self.assertRaises(AssertionError):
                demo_key_use.verify_result(dict(env, TQA_KEY_USE_AFTER="4"))
        # Nothing was broken (another suite, or a failure before the break): mending does nothing.
        with patch.object(compose, "command") as command:
            demo_key_use.mend_audit(["docker", "compose"], {})
            command.assert_not_called()

    def test_an_unknown_browser_is_refused_before_anything_starts(self):
        with patch.dict(demo.os.environ, {"TQA_DOCKER_TESTS": "1", "TQA_STACK_BROWSER": "firefox"}), \
                patch.object(demo.sys, "argv", ["demo.py", "--suite", "previews"]), \
                patch.object(compose, "preflight") as preflight:
            with self.assertRaisesRegex(SystemExit, "TQA_STACK_BROWSER must be one of chromium, webkit"):
                demo.main()
        preflight.assert_not_called()

    def test_a_failure_keeps_the_stack_logs_in_the_evidence_folder(self):
        with tempfile.TemporaryDirectory() as evidence, patch.object(compose, "save_logs",
                                                                     return_value=["written"]) as save:
            env = {"TQA_WEB_EVIDENCE_DIR": evidence}
            self.assertEqual(demo.keep_logs(env, AssertionError("failed"), demo.COMPOSE), ["written"])
        save.assert_called_once_with(env, Path(evidence) / "stack-logs", configuration=demo.COMPOSE)

    def test_without_an_evidence_folder_the_logs_outlive_the_temporary_installation(self):
        with tempfile.TemporaryDirectory() as parent, patch.object(demo.tempfile, "tempdir", parent), \
                patch.object(compose, "save_logs", side_effect=[["written"], []]) as save:
            demo.keep_logs({}, AssertionError("failed"), demo.COMPOSE)
            folder = save.call_args[0][1]
            self.assertTrue(folder.name.startswith("tqa-demo-logs-"))
            self.assertTrue(folder.is_dir())
            # A folder that received nothing is not left behind.
            demo.keep_logs({}, AssertionError("failed"), demo.COMPOSE)
            self.assertFalse(save.call_args[0][1].exists())

    def test_success_and_ctrl_c_keep_no_logs(self):
        with patch.object(compose, "save_logs") as save:
            self.assertEqual(demo.keep_logs({}, None, demo.COMPOSE), [])
            self.assertEqual(demo.keep_logs({}, KeyboardInterrupt(), demo.COMPOSE), [])
        save.assert_not_called()

    def test_logs_are_kept_before_the_sandboxes_are_removed(self):
        source = Path(demo.__file__).read_text()
        self.assertLess(source.index("kept = keep_logs("), source.index("compose.retire(env, failure"))

    def test_a_failed_check_is_named_in_the_log_redacted(self):
        def check(_env):
            raise FileNotFoundError("no result beside synthetic-password")

        errors = io.StringIO()
        with contextlib.redirect_stderr(errors):
            failure = demo.verify("files", check, {"TQA_POSTGRES_PASSWORD": "synthetic-password"})
        self.assertIsInstance(failure, FileNotFoundError)
        self.assertIn("Demo contract failed: FileNotFoundError: no result beside", errors.getvalue())
        self.assertNotIn("synthetic-password", errors.getvalue())

    def test_a_passing_check_is_no_failure(self):
        seen = []
        errors = io.StringIO()
        with contextlib.redirect_stderr(errors):
            self.assertIsNone(demo.verify("files", seen.append, {"TQA_STACK_SUITE": "files"}))
        self.assertEqual(seen, [{"TQA_STACK_SUITE": "files"}])
        self.assertEqual(errors.getvalue(), "")

    def test_a_run_narrowed_to_some_tests_skips_the_suites_check_and_says_so(self):
        seen = []
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertIsNone(demo.verify("files", seen.append, {"TQA_STACK_GREP": "WEB-E2E-027"}))
        self.assertEqual(seen, [])
        self.assertIn("Only the tests matching TQA_STACK_GREP ran, so the files suite's own check is skipped",
                      output.getvalue())

    def test_the_driver_checks_each_suite_through_verify(self):
        source = Path(demo.__file__).read_text()
        self.assertIn("failure = verify(args.suite, checks[args.suite], env)", source)

    def test_cleanup_uses_demo_overlay_and_preserves_original_failure(self):
        failure = AssertionError("browser failed")
        with patch.object(compose, "command", side_effect=OSError("down failed")) as command, \
                patch.object(compose, "remove_project") as removed, \
                patch.object(compose, "ownership"), tempfile.TemporaryDirectory() as directory, \
                contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(AssertionError) as caught:
                compose.cleanup(Path(directory), {"COMPOSE_PROJECT_NAME": "fixture"},
                                unittest.mock.Mock(), failure, compose=demo.COMPOSE)
        self.assertIs(caught.exception, failure)
        self.assertEqual(command.call_args[0][0][:6], demo.COMPOSE)
        removed.assert_called_once()


if __name__ == "__main__":
    unittest.main()
