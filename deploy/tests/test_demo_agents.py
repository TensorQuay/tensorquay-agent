"""Offline boundaries for E2E-AGENTS-020's driver side: the restart handshake and the run's verdict."""

import json
import re
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch

import compose
import demo_agents


def wait_for_gone(path, bound=5.0):
    """True once `path` no longer exists, within `bound` seconds."""
    deadline = time.monotonic() + bound
    while path.exists():
        if time.monotonic() > deadline:
            return False
        time.sleep(0.02)
    return True


def wait_for(path, bound=5.0):
    """True once `path` exists, within `bound` seconds."""
    deadline = time.monotonic() + bound
    while not path.exists():
        if time.monotonic() > deadline:
            return False
        time.sleep(0.05)
    return True


class RestarterTests(unittest.TestCase):
    def test_the_gateway_restarts_once_when_the_spec_asks_and_the_answer_names_it(self):
        with tempfile.TemporaryDirectory() as root, patch.object(compose, "command") as command:
            markers = Path(root) / "markers"
            restarter = demo_agents.Restarter(markers, ["docker", "compose"], {}).start()
            (markers / demo_agents.WANTED).write_text("after the fixer's report")
            self.assertTrue(wait_for(markers / demo_agents.DONE))
            self.assertEqual(restarter.finish(), "restarted")
            calls = [call.args[0] for call in command.call_args_list]
            self.assertEqual(calls[0], ["docker", "compose", "restart", "gateway"])
            self.assertEqual(calls[1][-1], "gateway")
            self.assertIn("--wait", calls[1])
            self.assertEqual(len(calls), 2, "restarted once")

    def test_a_second_request_restarts_again_once_the_spec_clears_the_first_answer(self):
        """E2E-AGENTS-020 asks once, then E2E-AGENTS-023 asks again while helpers work: each request is one restart."""
        with tempfile.TemporaryDirectory() as root, patch.object(compose, "command") as command:
            markers = Path(root) / "markers"
            restarter = demo_agents.Restarter(markers, ["docker", "compose"], {}).start()
            for reason in ("after the fixer's report", "while helpers work"):
                (markers / demo_agents.DONE).unlink(missing_ok=True)
                (markers / demo_agents.WANTED).write_text(reason)
                self.assertTrue(wait_for(markers / demo_agents.DONE))
                self.assertTrue(wait_for_gone(markers / demo_agents.WANTED))
            self.assertEqual(restarter.finish(), "restarted")
            self.assertEqual(restarter.restarts, 2)
            restarts = [call.args[0] for call in command.call_args_list if call.args[0][-2:] == ["restart", "gateway"]]
            self.assertEqual(len(restarts), 2)

    def test_a_failed_restart_is_written_for_the_spec_naming_why(self):
        with tempfile.TemporaryDirectory() as root, \
                patch.object(compose, "command", side_effect=RuntimeError("container gone")):
            markers = Path(root) / "markers"
            restarter = demo_agents.Restarter(markers, ["docker", "compose"], {}).start()
            (markers / demo_agents.WANTED).write_text("now")
            self.assertTrue(wait_for(markers / demo_agents.FAILED))
            self.assertIn("container gone", (markers / demo_agents.FAILED).read_text())
            self.assertFalse((markers / demo_agents.DONE).exists())
            self.assertIn("restart failed", restarter.finish())
            self.assertEqual(list(markers.glob("*.partial")), [], "markers are written whole, then moved in")

    def test_a_spec_that_never_asks_is_named_in_the_verdict(self):
        with tempfile.TemporaryDirectory() as root, patch.object(compose, "command") as command:
            restarter = demo_agents.Restarter(Path(root) / "markers", ["docker", "compose"], {}).start()
            self.assertEqual(restarter.finish(), "the spec never wrote restart-wanted")
            command.assert_not_called()

    def test_the_verdict_wants_both_runs_ended_by_done_with_one_end_note_and_the_restart(self):
        restarter = unittest.mock.Mock()
        restarter.finish.return_value = "restarted"
        run = {"stop_reason": "done", "end_notes": 1, "helpers": ["fixer", "analyst"]}
        with tempfile.TemporaryDirectory() as root:
            result = Path(root) / "browser.json"
            env = {"TQA_STACK_RESULT": str(result)}
            result.write_text(json.dumps({"runs": [dict(run, variant="plain"), dict(run, variant="restart")]}))
            demo_agents.verify_result(env, restarter)
            for broken in [{"end_notes": 2}, {"stop_reason": "budget"}, {"helpers": ["analyst"]}]:
                result.write_text(json.dumps({"runs": [dict(run, variant="plain"), dict(run, variant="restart", **broken)]}))
                with self.assertRaises(AssertionError, msg=str(broken)):
                    demo_agents.verify_result(env, restarter)
            restarter.finish.return_value = "the spec never wrote restart-wanted"
            restarter.outcome = "the spec never wrote restart-wanted"
            result.write_text(json.dumps({"runs": [dict(run, variant="plain"), dict(run, variant="restart")]}))
            with self.assertRaises(AssertionError):
                demo_agents.verify_result(env, restarter)

    def test_each_conversation_picks_only_its_own_script(self):
        """The fake model serves the first reply whose phrase is in a conversation's first message. A helper's first
        message is the gateway's frame ("You are <name>, a helper ...") and then its brief, so no brief may hold another
        conversation's phrase."""
        tests = Path(__file__).resolve().parent
        replies = json.loads((tests / "fixtures/agents.json").read_text())["responses"]
        spec = (tests.parent.parent / "web/e2e/agents-stack.spec.ts").read_text()
        ask = "".join(re.findall(r'"([^"]*)"', spec.split("const ASK =", 1)[1].split(";", 1)[0]))
        briefs = {call["arguments"]["role"]: f'You are {call["arguments"]["role"]}, a helper. '
                  + " ".join(v for v in call["arguments"]["brief"].values() if v)
                  for reply in replies for call in reply["events"] if call.get("name") == "start_member"}
        # The edge cases' leads (E2E-AGENTS-021..034): each person's message holds its own EDGE- phrase, named whole
        # since two leads may share a letter (EDGE-J's two teams).
        edges = {"lead " + when: when for when in {r["when"] for r in replies} if when.startswith("EDGE-")}
        firsts = dict(briefs, lead=ask, **edges)
        self.assertEqual(sorted(name for name in firsts if not name.startswith("lead ")),
                         ["analyst", "files-one", "files-two", "fixer", "ghost", "holder-k", "holder-one", "holder-two",
                          "late-files", "late-report", "lead", "noter", "profiled", "quiet", "reporter-l", "reporter-u",
                          "scribe-o", "slow-files", "spender", "writer"])
        # The holders share one script on purpose (held replies, for Stop, delete and restart); ghost has none, so the
        # fake model refuses it and its run fails (E2E-AGENTS-021); profiled never starts, as its profile doesn't
        # exist (E2E-AGENTS-034), so it has none either.
        family = {"holder-one": "holder", "holder-two": "holder", "holder-k": "holder"}
        for unscripted in ("ghost", "profiled"):
            self.assertFalse([r["when"] for r in replies if r["when"] in firsts[unscripted]],
                             f"{unscripted} must have no script")
        for reply in replies:
            picked = {family.get(name, name) for name, first in firsts.items()
                      if next((r["when"] for r in replies if r["when"] in first), None) == reply["when"]}
            self.assertEqual(len(picked), 1, (reply["when"], picked))

    def test_each_edge_phrase_in_the_spec_has_its_script(self):
        """A lead's script is picked by its EDGE- phrase: the specs' phrases and the fixture's must be the same set, or a
        lead has no script and never starts its helpers (the fifth edge run, 7 Oct: a renamed phrase)."""
        tests = Path(__file__).resolve().parent
        replies = json.loads((tests / "fixtures/agents.json").read_text())["responses"]
        specs = [tests.parent.parent / "web/e2e" / name
                 for name in ("agents-stack.spec.ts", "agents-edges-stack.spec.ts",
                              "agents-lead-unread-stack.spec.ts")]
        in_spec = {phrase for spec in specs for phrase in re.findall(r'"(EDGE-[^"]+)"', spec.read_text())}
        in_fixture = {reply["when"] for reply in replies if reply["when"].startswith("EDGE-")}
        self.assertEqual(in_spec, in_fixture)

    def test_every_tool_call_names_how_it_streams(self):
        """The fake model refuses a whole scenario with a tool call that leaves out `split_into` (1 to 64 pieces), so one
        such reply stops every agents case from starting (TK-U-050; the 16o integration: three replies of the T5/T7 cases
        left it out). Checked here too, so the fixture's own test catches it."""
        replies = json.loads((Path(__file__).resolve().parent / "fixtures/agents.json").read_text())["responses"]
        missing = [(index, reply["when"], event.get("name")) for index, reply in enumerate(replies)
                   for event in reply["events"] if event.get("type") == "tool_call"
                   and not (isinstance(event.get("split_into"), int) and not isinstance(event.get("split_into"), bool)
                            and 1 <= event["split_into"] <= 64)]
        self.assertEqual(missing, [])

    def test_every_brief_has_what_start_member_requires(self):
        """start_member's brief takes objective, output and boundaries as text (only sources may be null): a script
        that leaves one out is refused, and its lead goes on with no helper (the first edge run, 7 Oct)."""
        replies = json.loads((Path(__file__).resolve().parent / "fixtures/agents.json").read_text())["responses"]
        briefs = [(reply["when"], call["arguments"]["brief"]) for reply in replies for call in reply["events"]
                  if call.get("name") == "start_member"]
        self.assertTrue(briefs)
        for when, brief in briefs:
            for field in ("objective", "output", "boundaries"):
                self.assertIsInstance(brief.get(field), str, (when, field))
                self.assertTrue(brief[field].strip(), (when, field))

    def test_every_question_reference_names_what_the_gateway_takes(self):
        """The gateway takes an answer's `question_seq` only for a `question` (an ask_all), and a decision's for a
        `question` or a helper's `ask`: a script that names anything else is refused and stops there."""
        replies = json.loads((Path(__file__).resolve().parent / "fixtures/agents.json").read_text())["responses"]
        named = [(reply["when"], call["name"], call["arguments"].get("type"),
                  call["arguments"]["question_seq"]["seq_of"]["type"])
                 for reply in replies for call in reply["events"]
                 if isinstance(call.get("arguments", {}).get("question_seq"), dict)]
        self.assertEqual(sorted(name for _, name, _, _ in named), ["decide", "send_to_lead"], named)
        for when, name, post, kind in named:
            allowed = ["question", "ask"] if name == "decide" else ["question"]
            self.assertIn(kind, allowed, (when, name, post))

    def test_no_script_ends_on_a_question_to_the_person(self):
        """A script's last reply repeats on each further call, and ask_person doesn't end the turn: a script ending on it
        has its lead ask again and again with the same result, so the runner's no-progress warning shows after 6 such
        steps and the run fails at 12, by how long the spec takes to stop it (c17's final shots, S4: EDGE-R). Each
        script ends its turn with words after the question."""
        replies = json.loads((Path(__file__).resolve().parent / "fixtures/agents.json").read_text())["responses"]
        last = {}
        for reply in replies:
            last[reply["when"]] = reply
        ending = [when for when, reply in last.items()
                  if any(event.get("name") == "ask_person" for event in reply["events"])]
        self.assertEqual(ending, [])

    def test_a_failed_runs_team_state_is_kept_and_reading_it_never_raises(self):
        with tempfile.TemporaryDirectory() as root:
            answer = unittest.mock.Mock(stdout=" name | state\n fixer | working\n")
            with patch.object(compose, "command", return_value=answer) as command:
                demo_agents.save_team_state({}, root, ["docker", "compose"])
            self.assertEqual(command.call_args.args[0][:6], ["docker", "compose", "exec", "-T", "postgres", "psql"])
            self.assertIn("fixer | working", (Path(root) / "team-state.txt").read_text())
            with patch.object(compose, "command", side_effect=RuntimeError("no database")):
                demo_agents.save_team_state({}, Path(root) / "other", ["docker", "compose"])
            self.assertFalse((Path(root) / "other").exists())

    def test_the_deadline_scale_reads_as_the_harness_does(self):
        for value, expected in [(None, 1), ("4", 4), (" 20 ", 20), ("21", 1), ("0", 1), ("x", 1)]:
            env = {} if value is None else {"TQA_TEST_DEADLINE_SCALE": value}
            self.assertEqual(demo_agents.scale(env), expected, value)


if __name__ == "__main__":
    unittest.main()
