"""E2E-TEAM-011: the Gate 4 demo's replies, checks and transcript, without a stack.

Run: python3 -m unittest discover -s tests/e2e -p 'test_*.py' -v
"""

import json
import unittest

import gate4_evidence as evidence

COMMIT = "a" * 40


def frame(kind, second, **event):
    return {"type": "event", "ts": "2026-09-26T10:00:{:02d}Z".format(second), "event": dict(event, type=kind)}


def run(start, end):
    return [frame("run.started", start, run_id="r"), frame("run.completed", end, run_id="r")]


def pushed(status="ok"):
    arguments = {"command": evidence.PUSH + "demo/a"}
    return [frame("tool.call", 1, call_id="c1", name="shell", arguments=arguments),
            frame("tool.result", 4, call_id="c1", status=status, content="pushed\nmore")]


class RepliesTests(unittest.TestCase):
    def scripts(self):
        by_task = {}
        for response in evidence.replies()["responses"]:
            by_task.setdefault(response["when"], []).append(response)
        return by_task

    def test_each_run_follows_its_own_script_by_its_message(self):
        self.assertEqual(evidence.replies()["select"], "prompt")
        steps = {when: [response["events"][0].get("name") or "text" for response in replies]
                 for when, replies in self.scripts().items()}
        self.assertEqual(steps, {evidence.MESSAGE_A: ["shell", "shell", "text"],
                                 evidence.MESSAGE_FORK: ["shell", "shell", "text"],
                                 evidence.MESSAGE_WAIT: ["text"]})

    def test_only_the_holders_push_is_held(self):
        # A's run keeps the workspace lease through its held push step while B forks and pushes (ADR-0035: there
        # is no approval to hold it at).
        held = [(when, index) for when, replies in self.scripts().items()
                for index, response in enumerate(replies) if "wait_ms" in response]
        self.assertEqual(held, [(evidence.MESSAGE_A, 1)])
        self.assertEqual(self.scripts()[evidence.MESSAGE_A][1]["wait_ms"], evidence.HOLD_MS)

    def test_each_push_is_plain_git_with_the_persons_key_to_its_branch(self):
        calls = [event["arguments"]["command"] for response in evidence.replies()["responses"]
                 for event in response["events"] if evidence.is_push(event.get("arguments", {}))]
        self.assertEqual(calls, [evidence.PUSH + "demo/a", evidence.PUSH + "demo/b"])
        # The key's placeholder, never a value; and no retired git_push call is left.
        self.assertIn("${GATE4_ORIGIN}", evidence.PUSH)
        names = [event.get("name") for response in evidence.replies()["responses"] for event in response["events"]]
        self.assertNotIn("git_push", names)


class OrderTests(unittest.TestCase):
    def test_the_fork_runs_inside_the_holder_and_the_waiting_run_follows(self):
        order = evidence.check_order(run(1, 30), run(5, 20), run(31, 40))
        self.assertEqual(order["fork"], ["2026-09-26T10:00:05+00:00", "2026-09-26T10:00:20+00:00"])

    def test_a_fork_run_outside_the_holder_fails(self):
        with self.assertRaisesRegex(AssertionError, "fork run"):
            evidence.check_order(run(1, 10), run(5, 20), run(31, 40))

    def test_a_waiting_run_that_starts_early_fails(self):
        with self.assertRaisesRegex(AssertionError, "waiting run"):
            evidence.check_order(run(1, 30), run(5, 20), run(25, 40))

    def test_a_run_that_never_completed_fails(self):
        with self.assertRaisesRegex(AssertionError, "did not start and complete"):
            evidence.check_order(run(1, 30)[:1], run(5, 20), run(31, 40))


class PushTests(unittest.TestCase):
    def test_one_push_that_succeeded_passes(self):
        self.assertEqual(evidence.check_push(pushed(), "demo/a"), {"calls": 1, "results": ["ok"]})

    def test_a_shell_call_that_isnt_the_push_is_not_counted(self):
        other = [frame("tool.call", 1, call_id="c9", name="shell", arguments={"command": "git status"}),
                 frame("tool.result", 2, call_id="c9", status="ok", content="")]
        self.assertEqual(evidence.push_outcomes(other + pushed()), {"calls": 1, "results": ["ok"]})

    def test_a_failed_or_repeated_push_fails(self):
        for stored in (pushed(status="error"), pushed() + pushed()):
            with self.assertRaises(AssertionError):
                evidence.check_push(stored, "demo/a")

    def test_both_branches_must_exist_at_the_commit(self):
        refs = {"refs/heads/main": COMMIT, "refs/heads/demo/a": COMMIT, "refs/heads/demo/b": COMMIT}
        self.assertEqual(len(evidence.check_refs(refs, COMMIT)), 3)
        for broken in ({**refs, "refs/heads/demo/b": "b" * 40}, {k: v for k, v in refs.items() if "demo/a" not in k}):
            with self.assertRaisesRegex(AssertionError, "both branches"):
                evidence.check_refs(broken, COMMIT)


class RecordTests(unittest.TestCase):
    def test_secrets_and_tokens_are_refused_in_any_record(self):
        evidence.check_clean({"events": "nothing here"}, ["synthetic-secret", None])
        with self.assertRaisesRegex(AssertionError, "secret reached the recorded events"):
            evidence.check_clean({"events": "x synthetic-secret"}, ["synthetic-secret"])
        with self.assertRaisesRegex(AssertionError, "API token"):
            evidence.check_clean({"result": "tqa_pat_abc"}, [])

    def test_the_transcript_lists_the_shown_events_and_the_observations(self):
        stored = [frame("message.user", 0, text="Push it", attachments=[]), frame("step.started", 0, step=1)]
        stored += pushed() + [frame("text.delta", 5, text="x" * 400)]
        text = evidence.transcript([("A", stored)], ["B was queued"])
        self.assertIn("- `2026-09-26T10:00:00Z` message.user — Push it", text)
        self.assertIn("tool.call — shell {\"command\": \"git -c", text)
        self.assertIn("${GATE4_ORIGIN}", text)
        self.assertIn("tool.result — ok: pushed\n", text)
        self.assertNotIn("step.started", text)
        self.assertIn("…", text)
        self.assertTrue(text.endswith("## Observations\n\n- B was queued\n"))

    def test_the_result_is_bounded(self):
        self.assertEqual(json.loads(evidence.result({"a": 1})), {"a": 1})
        with self.assertRaisesRegex(AssertionError, "bound"):
            evidence.result({"notes": ["x" * evidence.RESULT_BYTES]})


if __name__ == "__main__":
    unittest.main()
