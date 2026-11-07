"""E2E-CHANNEL-016's script and checks without a stack. Run: python3 -m unittest tests/e2e/test_gate7.py"""

import copy
import json
import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gate7_evidence as evidence  # noqa: E402

NAMES = {"c-planner": "Planner", "c-builder": "Builder"}
IDS = {"Planner": "c-planner", "Builder": "c-builder"}


def author(who):
    if who == "gateway":
        return {"kind": "gateway", "id": "gateway", "name": "gateway"}
    if who == "person":
        return {"kind": "user", "id": "u-1", "name": evidence.PERSON_EMAIL}
    return {"kind": "conversation", "id": IDS[who], "name": who}


def scripted_log():
    return [{"seq": seq, "ts": "2026-09-28T00:00:00Z", "author": author(who), "body": dict(fields, type=kind)}
            for seq, kind, who, fields in evidence.LOG]


def event(kind, **fields):
    return {"type": "event", "event": dict(fields, type=kind)}


def run_events(agent):
    events = [event("run.started")]
    for name, arguments in evidence.CALLS[agent]:
        events.append(event("tool.call", call_id="x", name=name, arguments=arguments))
    return events + [event("run.completed")]


class Script(unittest.TestCase):
    def test_the_log_is_contiguous_and_its_references_point_backwards(self):
        seqs = [seq for seq, _, _, _ in evidence.LOG]
        self.assertEqual(seqs, list(range(1, len(seqs) + 1)))
        kinds = {seq: kind for seq, kind, _, _ in evidence.LOG}
        for seq, _, _, fields in evidence.LOG:
            for key, target in (("proposal_seq", "proposal"), ("task_seq", "task")):
                if key in fields:
                    self.assertLess(fields[key], seq)
                    self.assertEqual(kinds[fields[key]], target, (seq, key))

    def test_the_script_file_is_the_words_the_log_names(self):
        words = json.loads(evidence.script_file())
        self.assertEqual(words["agents"], ["Planner", "Builder"])
        self.assertEqual(evidence.LOG[1][3]["title"], words["proposal"])
        self.assertEqual(evidence.LOG[8][3]["title"], words["task"])

    def test_profiles_leave_the_tools_open_on_their_own_route(self):
        # A profile's allow-list can't name the channel tools yet (the gateway checks it against the built-in names),
        # so the profiles have none; the runtime registers the channel tools for a member conversation, and C4 checks
        # that the agents called only those.
        for agent in evidence.AGENTS:
            profile = evidence.profile(agent)
            self.assertNotIn("tools", profile)
            self.assertEqual((profile["display_name"], profile["model_route"]), (agent, evidence.ROUTES[agent]))

    def test_a_refusal_names_its_status_and_body(self):
        text = evidence.refusal("/api/v1/profiles", 400, b'{"error":{"code":"invalid_request","details":[{"field":"tools"}]}}')
        self.assertIn("/api/v1/profiles", text)
        self.assertIn("400", text)
        self.assertIn('"field":"tools"', text)


class Checks(unittest.TestCase):
    def test_the_scripted_log_passes_and_any_difference_fails(self):
        self.assertEqual(evidence.check_log(scripted_log(), NAMES), {"messages": 14})
        swapped = scripted_log()
        swapped[3]["author"], swapped[4]["author"] = swapped[4]["author"], swapped[3]["author"]
        wrong_word = scripted_log()
        wrong_word[8]["body"]["claims"] = 1
        for broken in (scripted_log()[:-1], swapped, wrong_word):
            with self.subTest(len(broken)), self.assertRaises(AssertionError):
                evidence.check_log(broken, NAMES)

    def test_decisions(self):
        accepted = {"state": "decided", "decision": {"seq": 6, "outcome": "accepted"},
                    "tally": {"accept": 2, "reject": 0}, "voting_members": 3}
        vetoed = {"state": "decided", "decision": {"seq": 8, "outcome": "vetoed"}}
        self.assertEqual(evidence.check_decisions(accepted, vetoed), {"accepted": 2, "vetoed": 3})
        timed_out = copy.deepcopy(accepted)
        timed_out["decision"]["outcome"] = "timed_out"
        with self.assertRaises(AssertionError):
            evidence.check_decisions(timed_out, vetoed)

    def test_the_board(self):
        board = {"items": [{"task_seq": 9, "title": evidence.SCRIPT["task"], "limit": 2,
                            "claims": [{"author": author("Planner"), "seq": 10}, {"author": author("person"), "seq": 12}],
                            "reports": [{"author": author("Planner"), "seq": 11, "status": "completed"},
                                        {"author": author("person"), "seq": 13, "status": "completed"}]}]}
        self.assertEqual(evidence.check_board(board, NAMES), {"claims": 2, "reports": 2})
        builder_claimed = copy.deepcopy(board)
        builder_claimed["items"][0]["claims"][0]["author"] = author("Builder")
        with self.assertRaises(AssertionError):
            evidence.check_board(builder_claimed, NAMES)

    def test_runs(self):
        for agent in evidence.AGENTS:
            self.assertEqual(evidence.check_run(agent, run_events(agent)), {agent: len(evidence.CALLS[agent])})
        failed = run_events("Builder")[:-1] + [event("run.failed")]
        extra = run_events("Builder")
        extra.insert(3, event("tool.call", call_id="y", name="task_claim", arguments={"task_seq": 9}))
        for broken in (failed, extra):
            with self.subTest(len(broken)), self.assertRaises(AssertionError):
                evidence.check_run("Builder", broken)

    def test_the_page(self):
        seen = {"proposal_seq": 2, "decision_seq": 6, "veto_seq": 7, "task_seq": 9, "order": list(range(1, 15))}
        self.assertEqual(evidence.check_page(seen), {"page_seen": 14})
        for key, value in (("veto_seq", 8), ("order", [1, 3, 2])):
            broken = dict(seen, **{key: value})
            with self.subTest(key), self.assertRaises(AssertionError):
                evidence.check_page(broken)


if __name__ == "__main__":
    unittest.main()


class Scenarios(unittest.TestCase):
    """The fake-model scenarios (gate7_scenarios.py) make exactly the calls the driver checks, with the script's words."""

    def calls(self, scenario):
        found = []
        for response in scenario["responses"]:
            for output in response["events"]:
                if output["type"] == "tool_call":
                    arguments = output["arguments"]
                    found.append((output["name"], json.loads(arguments) if isinstance(arguments, str) else arguments))
        return found

    def test_each_agent_makes_the_checked_calls_with_the_scripts_words_and_no_parent(self):
        import gate7_scenarios  # noqa: PLC0415
        scenarios = gate7_scenarios.scenarios("01a0e2c2-0000-7000-8000-000000000000")
        self.assertEqual(sorted(scenarios), sorted(evidence.AGENTS))
        posted = {}
        for agent in evidence.AGENTS:
            calls = self.calls(scenarios[agent])
            events = [{"type": "event", "event": {"type": "tool.call", "name": name, "arguments": arguments}}
                      for name, arguments in calls] + [{"type": "event", "event": {"type": "run.completed"}}]
            evidence.check_run(agent, events)
            for name, arguments in calls:
                self.assertNotIn("parent", arguments, (agent, name))
                if name == "channel_post":
                    posted.setdefault(agent, []).append(arguments)
        # Every agent message in the log, with its scripted words.
        # Claims are made by task_claim, not posted.
        want = [(who, kind, fields) for _, kind, who, fields in evidence.LOG if who in evidence.AGENTS and kind != "claim"]
        got = [(agent, arguments["type"], arguments) for agent in evidence.AGENTS for arguments in posted[agent]]
        self.assertEqual(sorted((who, kind) for who, kind, _ in want), sorted((who, kind) for who, kind, _ in got))
        for who, kind, fields in want:
            match = [arguments for agent, posted_kind, arguments in got if agent == who and posted_kind == kind]
            self.assertTrue(any(all(arguments.get(key) == value for key, value in fields.items()) for arguments in match),
                            (who, kind, fields, match))


class ScenarioWriter(unittest.TestCase):
    def test_a_scenario_is_rewritten_by_its_owner_in_a_container_that_sees_only_that_file(self):
        args = evidence.scenario_writer(Path("/tmp/x/installation/planner.json"), "toybox@sha256:abc")
        self.assertEqual(args[:3], ["docker", "run", "--rm"])
        self.assertIn("-i", args)
        self.assertEqual(args[args.index("--network") + 1], "none")
        self.assertEqual(args[args.index("--user") + 1], "1000:1000")
        mount = args[args.index("--mount") + 1]
        self.assertEqual(mount, "type=bind,source=/tmp/x/installation/planner.json,target=/scenario.json")
        self.assertEqual(args[-3:], ["toybox@sha256:abc", "tee", "/scenario.json"])


class PageFiles(unittest.TestCase):
    def test_the_page_files_live_beside_the_installation_not_in_it(self):
        installation = Path("/tmp/tqa-gate7-x/installation")
        script, result = evidence.page_files(installation)
        self.assertEqual((script.parent, result.parent), (installation.parent, installation.parent))
        self.assertEqual((script.name, result.name), ("gate7-script.json", "channel-page.json"))
