"""E2E-CMP-003's runner without Docker or a model: task selection, the containers' arguments, the reference API
adapter, one attempt end to end against fakes, and the records. Run: python3 -m unittest tests/e2e/cmp/test_cmp.py"""

import json
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import reference  # noqa: E402
import runner  # noqa: E402

TASKS = HERE.parent / "tasks"
DIGEST = "registry.invalid/reference@sha256:" + "a" * 64
MODEL_KEY = "model-fixture-marker"


class Tasks(unittest.TestCase):
    def test_selects_the_tagged_tasks_with_their_budgets(self):
        tasks = runner.select_tasks(TASKS, "cmp")
        self.assertEqual(len(tasks), 10)
        by_id = {task.id: task for task in tasks}
        self.assertEqual((by_id["py-rename-refactor"].max_steps, by_id["py-rename-refactor"].wall_clock_secs),
                         (40, 900))
        self.assertEqual(by_id["ts-config-npm-test"].template, "workstation")
        self.assertEqual(by_id["py-fix-off-by-one"].template, "default")
        self.assertEqual(by_id["py-fix-off-by-one"].max_tokens, 200000)
        self.assertEqual([task.id for task in tasks], sorted(task.id for task in tasks))

    def test_only_narrows_the_tagged_tasks(self):
        tasks = runner.select_tasks(TASKS, "cmp", only=["py-fix-off-by-one"])
        self.assertEqual([task.id for task in tasks], ["py-fix-off-by-one"])
        with self.assertRaises(SystemExit):
            runner.select_tasks(TASKS, "cmp", only=["py-docs-api-reference"])  # not tagged cmp
        with self.assertRaises(SystemExit):
            runner.select_tasks(TASKS, "cmp", only=["no-such-task"])

    def test_the_task_text_is_task_md_verbatim(self):
        task = {t.id: t for t in runner.select_tasks(TASKS, "cmp")}["py-fix-off-by-one"]
        self.assertEqual(task.text, (TASKS / "py-fix-off-by-one" / "task.md").read_text())


class Order(unittest.TestCase):
    def test_attempts_run_in_the_harness_order(self):
        # tqa-e2e's schedule: each task's attempts together, tasks in id order, so both sides meet the server alike.
        tasks = runner.select_tasks(TASKS, "cmp")[:2]
        self.assertEqual([(task.id, number) for task, number in runner.order(tasks, 3)],
                         [(tasks[0].id, 1), (tasks[0].id, 2), (tasks[0].id, 3),
                          (tasks[1].id, 1), (tasks[1].id, 2), (tasks[1].id, 3)])


class Images(unittest.TestCase):
    def test_the_reference_image_must_be_pinned(self):
        runner.require_pinned(DIGEST)
        runner.require_pinned("sha256:" + "b" * 64)
        for loose in ("registry.invalid/reference:1.0", "reference", "reference@sha256:short"):
            with self.assertRaises(SystemExit, msg=loose):
                runner.require_pinned(loose)


class Containers(unittest.TestCase):
    def test_the_reference_runs_on_the_internal_network_only(self):
        args = runner.reference_args("tqa-cmp-1", DIGEST, Path("/abs/workspace"), "tqa-a-model", "session-fixture")
        self.assertEqual(args[:2], ["run", "-d"])
        self.assertIn("--network", args)
        self.assertEqual(args[args.index("--network") + 1], "tqa-a-model")
        self.assertEqual(args.count("--network"), 1)
        for published in ("-p", "--publish", "-P"):
            self.assertNotIn(published, args)
        self.assertIn("type=bind,source=/abs/workspace,target=" + reference.WORKDIR, args)
        self.assertEqual(args[-1], DIGEST)
        # The model key never goes into the container's environment; only the server's session key does.
        self.assertFalse(any(MODEL_KEY in arg for arg in args))
        self.assertIn(reference.SESSION_ENV + "=session-fixture", args)

    def test_the_forwarder_publishes_on_loopback_only(self):
        args = runner.forward_args("tqa-cmp-fwd-1", "sha256:" + "c" * 64, "tqa-cmp-1")
        published = args[args.index("-p") + 1]
        self.assertTrue(published.startswith("127.0.0.1::"), published)
        self.assertIn("tqa-cmp-1", " ".join(args))


class Adapter(unittest.TestCase):
    def test_start_body_carries_the_task_the_model_and_the_step_limit(self):
        body = reference.start_body("Fix it.\n", "http://local-model:8000/v1", "served-model", MODEL_KEY, 30)
        self.assertEqual(body["max_iterations"], 30)
        self.assertEqual(body["workspace"]["working_dir"], reference.WORKDIR)
        self.assertEqual(body["initial_message"]["content"], [{"type": "text", "text": "Fix it.\n"}])
        self.assertTrue(body["initial_message"]["run"])
        llm = body["agent"]["llm"]
        self.assertEqual((llm["base_url"], llm["api_key"]), ("http://local-model:8000/v1", MODEL_KEY))
        self.assertTrue(llm["model"].endswith("served-model"))
        # The shapes the reference's own API description (1.49.6) gives: discriminated agent and workspace, tools by
        # their registered names with parameters, a typed text part.
        self.assertEqual((body["agent"]["kind"], body["workspace"]["kind"]), ("Agent", "LocalWorkspace"))
        self.assertEqual([tool["name"] for tool in body["agent"]["tools"]], ["terminal", "file_editor", "task_tracker"])
        self.assertTrue(all(tool["params"] == {} for tool in body["agent"]["tools"]))

    def test_tokens_sum_every_usage_id_and_fall_back_to_metrics(self):
        stats = {"stats": {"usage_to_metrics": {
            "agent": {"accumulated_token_usage": {"prompt_tokens": 100, "completion_tokens": 10}},
            "condenser": {"accumulated_token_usage": {"prompt_tokens": 5, "completion_tokens": 1}}}}}
        self.assertEqual(reference.tokens(stats), {"prompt": 105, "completion": 11})
        metrics = {"metrics": {"accumulated_token_usage": {"prompt_tokens": 9, "completion_tokens": 2}}}
        self.assertEqual(reference.tokens(metrics), {"prompt": 9, "completion": 2})
        self.assertIsNone(reference.tokens({"metrics": None, "stats": {}}))
        self.assertIsNone(reference.tokens(None))

    def test_the_model_settings_match_our_route(self):
        # Our route `local` sends an output cap and a chat-template switch; the reference sends the same, and leaves
        # sampling and reasoning effort to the server, as our route does.
        body = reference.start_body("Fix it.\n", "http://local-model:8000/v1", "served-model", MODEL_KEY, 30,
                                    max_output=16384, extra_body={"chat_template_kwargs": {"enable_thinking": True}})
        llm = body["agent"]["llm"]
        self.assertEqual(llm["max_output_tokens"], 16384)
        self.assertEqual(llm["litellm_extra_body"], {"chat_template_kwargs": {"enable_thinking": True}})
        for key in ("reasoning_effort", "temperature", "top_p"):
            self.assertIn(key, llm)
            self.assertIsNone(llm[key], key)

    def test_terminal_statuses(self):
        for status in ("finished", "error", "stuck"):
            self.assertTrue(reference.terminal(status), status)
        for status in ("idle", "running", "paused", "waiting_for_confirmation", None):
            self.assertFalse(reference.terminal(status), status)

    def test_redact_removes_the_model_key_everywhere(self):
        info = {"agent": {"llm": {"api_key": MODEL_KEY, "nested": ["x " + MODEL_KEY]}}}
        self.assertNotIn(MODEL_KEY, json.dumps(runner.redact(info, [MODEL_KEY])))


class Scoring(unittest.TestCase):
    def test_classes(self):
        self.assertEqual(runner.classify("finished", "passed", False), "completed")
        self.assertEqual(runner.classify("finished", "failed", False), "model")
        self.assertEqual(runner.classify("finished", "tampered", False), "model")
        self.assertEqual(runner.classify("stuck", "failed", False), "model")
        self.assertEqual(runner.classify("error", "failed", False), "harness")
        self.assertEqual(runner.classify("finished", "broken", False), "harness")
        self.assertEqual(runner.classify("running", "failed", True), "wall_clock")
        self.assertEqual(runner.classify(None, None, False), "setup")

    def test_wilson(self):
        low, high = runner.wilson(30, 30)
        self.assertAlmostEqual(low, 0.886, places=3)
        self.assertEqual(high, 1.0)
        self.assertEqual(runner.wilson(0, 0), (0.0, 0.0))


class FakeReference:
    """The reference agent server's API as the adapter uses it: a session key, start, then status polls."""

    def __init__(self, statuses, session_key, error_kind=None):
        self.statuses = list(statuses)
        self.started = []
        self.error_kind = error_kind
        fake = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def answer(self, status, body):
                data = json.dumps(body).encode()
                self.send_response(status)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def do_GET(self):
                if self.path == reference.HEALTH:
                    return self.answer(200, {"status": "ok"})
                if self.headers.get(reference.SESSION_HEADER) != session_key:
                    return self.answer(401, {"detail": "unauthorized"})
                if "/events/search" in self.path:
                    assert "kind=ConversationErrorEvent" in self.path, self.path
                    error = {"kind": "ConversationErrorEvent", "code": "X", "detail": "d",
                             "classification": {"kind": fake.error_kind, "retryable": False}}
                    items = [error] if fake.error_kind else []
                    return self.answer(200, {"items": items, "next_page_id": None})
                status = fake.statuses.pop(0) if len(fake.statuses) > 1 else fake.statuses[0]
                self.answer(200, {"id": "conv-1", "execution_status": status,
                                  "agent": {"llm": {"api_key": MODEL_KEY}}, "metrics": None,
                                  # Where the reference's server (1.49.6) reports usage: per LLM usage id.
                                  "stats": {"usage_to_metrics": {"agent": {"accumulated_token_usage": {
                                      "prompt_tokens": 900, "completion_tokens": 80, "reasoning_tokens": 7}}}}})

            def do_POST(self):
                if self.headers.get(reference.SESSION_HEADER) != session_key:
                    return self.answer(401, {"detail": "unauthorized"})
                fake.started.append(json.loads(self.rfile.read(int(self.headers["content-length"]))))
                self.answer(201, {"id": "conv-1", "execution_status": "running"})

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.port = self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()


class FakeDocker:
    """Records docker commands; the forwarder's port is the fake reference's."""

    def __init__(self, port):
        self.port = port
        self.calls = []

    def __call__(self, args, check=True):
        self.calls.append(list(args))
        if args[0] == "port":
            return "127.0.0.1:{}\n".format(self.port)
        return ""


class Attempt(unittest.TestCase):
    def setUp(self):
        self.out = Path(tempfile.mkdtemp())
        self.task = {t.id: t for t in runner.select_tasks(TASKS, "cmp")}["py-fix-off-by-one"]
        self.seeded, self.checked = [], []

    def seed(self, task, workspace):
        self.seeded.append(task.id)
        workspace.mkdir(parents=True)

    def check(self, task, workspace, record):
        self.checked.append(task.id)
        return {"outcome": "passed", "note": None, "dropped": 0}

    def settings(self, docker):
        return runner.Settings(reference_image=DIGEST, workstation_reference_image=None,
                               forward_image="sha256:" + "c" * 64, network="tqa-a-model",
                               model_url="http://local-model:8000/v1", model="served-model", model_key=MODEL_KEY,
                               poll_secs=0.01, margin_secs=60, docker=docker, seed=self.seed, check=self.check)

    def test_one_attempt_runs_checks_records_and_always_stops_its_containers(self):
        fake = FakeReference(["running", "finished"], session_key="session-fixture")
        self.addCleanup(fake.close)
        docker = FakeDocker(fake.port)
        settings = self.settings(docker)
        original = runner.new_session_key
        runner.new_session_key = lambda: "session-fixture"
        self.addCleanup(setattr, runner, "new_session_key", original)
        line = runner.run_attempt(self.task, 1, self.out, settings)
        self.assertEqual((line["task"], line["attempt"], line["class"], line["outcome"]),
                         ("py-fix-off-by-one", 1, "completed", "passed"))
        self.assertEqual(line["status"], "finished")
        self.assertEqual(line["tokens"], {"prompt": 900, "completion": 80})
        self.assertEqual(fake.started[0]["max_iterations"], 30)
        self.assertEqual(self.seeded, ["py-fix-off-by-one"])
        self.assertEqual(self.checked, ["py-fix-off-by-one"])
        removed = [call for call in docker.calls if call[:2] == ["rm", "-f"]]
        self.assertEqual(len(removed), 1)
        self.assertEqual(set(removed[0][2:]), {"tqa-cmp-py-fix-off-by-one-1", "tqa-cmp-fwd-py-fix-off-by-one-1"})
        saved = (self.out / "attempts" / "py-fix-off-by-one-1" / "conversation.json").read_text()
        self.assertNotIn(MODEL_KEY, saved)

    def test_the_wall_clock_stops_an_attempt_and_its_containers(self):
        fake = FakeReference(["running"], session_key="session-fixture")
        self.addCleanup(fake.close)
        docker = FakeDocker(fake.port)
        settings = self.settings(docker)
        original = runner.new_session_key
        runner.new_session_key = lambda: "session-fixture"
        self.addCleanup(setattr, runner, "new_session_key", original)
        task = self.task._replace(wall_clock_secs=0)
        settings = settings._replace(margin_secs=0.2)
        line = runner.run_attempt(task, 2, self.out, settings)
        self.assertEqual(line["class"], "wall_clock")
        self.assertTrue(any(call[:2] == ["rm", "-f"] for call in docker.calls))
        # A stopped attempt is still checked: its workspace is what it left.
        self.assertEqual(self.checked, ["py-fix-off-by-one"])


class Budget(unittest.TestCase):
    def test_an_attempt_past_its_token_budget_is_stopped_as_ours_would_be(self):
        fake = FakeReference(["running"], session_key="session-fixture")
        self.addCleanup(fake.close)
        docker = FakeDocker(fake.port)
        attempt = Attempt()
        attempt.setUp()
        original = runner.new_session_key
        runner.new_session_key = lambda: "session-fixture"
        self.addCleanup(setattr, runner, "new_session_key", original)
        # The fake reports 980 tokens; a budget of 500 is spent at the first poll.
        task = attempt.task._replace(max_tokens=500)
        line = runner.run_attempt(task, 1, attempt.out, attempt.settings(docker))
        self.assertEqual(line["class"], "model")
        self.assertTrue(line["budget_stop"])
        self.assertFalse(line["stopped"])
        self.assertTrue(any(call[:2] == ["rm", "-f"] for call in docker.calls))
        self.assertEqual(attempt.checked, ["py-fix-off-by-one"])


class Reruns(unittest.TestCase):
    """The exact mirror of our harness: a rerun only when the model server or the connection failed."""

    def error_attempt(self, kind):
        fake = FakeReference(["error"], session_key="session-fixture", error_kind=kind)
        self.addCleanup(fake.close)
        attempt = Attempt()
        attempt.setUp()
        original = runner.new_session_key
        runner.new_session_key = lambda: "session-fixture"
        self.addCleanup(setattr, runner, "new_session_key", original)
        return runner.run_attempt(attempt.task, 1, attempt.out, attempt.settings(FakeDocker(fake.port)))

    def test_a_provider_error_is_named_from_the_reference_event_log(self):
        for kind in ("transient", "rate_limit"):
            line = self.error_attempt(kind)
            self.assertEqual((line["class"], line["provider"], line["error_kind"]), ("harness", True, kind), kind)

    def test_any_other_error_is_the_reference_own_failure(self):
        for kind, cls in (("agent_action", "model"), ("quota", "model"), ("internal", "harness"),
                          ("unknown", "harness"), ("config", "harness"), ("auth", "harness"), (None, "harness")):
            line = self.error_attempt(kind)
            self.assertEqual((line["class"], line["provider"]), (cls, False), kind)

    def test_only_a_provider_failure_gets_one_rerun(self):
        tasks = runner.select_tasks(TASKS, "cmp")[:3]
        runs = {(tasks[0].id, "1"): ("harness", True), (tasks[0].id, "1r"): ("completed", False),
                (tasks[1].id, "1"): ("harness", True), (tasks[1].id, "1r"): ("harness", True),
                (tasks[2].id, "1"): ("harness", False)}
        ran = []

        def attempt(task, number, out, settings):
            ran.append((task.id, str(number)))
            cls, provider = runs.get((task.id, str(number)), ("completed", False))
            return {"task": task.id, "attempt": number, "class": cls, "provider": provider}

        lines = runner.run_all(tasks, 1, Path("unused"), None, attempt)
        self.assertEqual(ran, [(tasks[0].id, "1"), (tasks[0].id, "1r"), (tasks[1].id, "1"), (tasks[1].id, "1r"),
                               (tasks[2].id, "1")])
        self.assertEqual([line.get("rerun_of") for line in lines], [None, 1, None, 1, None])
        summary = runner.summary(lines)
        # Completion counts each attempt's final run; every harness failure is counted, a rerun never hides one.
        self.assertIn("1/3 completed", summary)
        self.assertIn("4 harness failures", summary)


class Records(unittest.TestCase):
    def test_summary_counts_completion_and_leaves_setup_failures_out(self):
        lines = [
            {"task": "a", "attempt": 1, "class": "completed"},
            {"task": "a", "attempt": 2, "class": "model"},
            {"task": "b", "attempt": 1, "class": "completed"},
            {"task": "b", "attempt": 2, "class": "setup"},
        ]
        summary = runner.summary(lines)
        self.assertIn("2/3 completed", summary)
        self.assertIn("1 setup failure (not counted; rerun)", summary)
        self.assertIn("| a | 1/2 |", summary)


if __name__ == "__main__":
    unittest.main()
