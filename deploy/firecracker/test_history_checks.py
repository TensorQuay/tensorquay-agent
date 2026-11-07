"""SEC-017 on the microVM: the judgements over what the second guest and its VM host show (no KVM)."""
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import history_checks as checks  # noqa: E402

FIRST = "01a0e4cd-0b0a-7345-8efa-f4187ce26497"
SECOND = "01a0e4cd-1e63-76a6-af55-48bcd252251b"


def probe(**fields):
    record = {"state": ["events.wal"], "state_mounts": [["/dev/vdc", "/run/tqa/state"]], "ids": [], "samples": [],
              "forged": ["/workspace/events.jsonl"]}
    record.update(fields)
    return "noise\n" + checks.PROBE_MARK + json.dumps(record) + "\n"


class Guest(unittest.TestCase):
    def test_one_own_state_device_and_nothing_of_the_first_passes(self):
        self.assertEqual(checks.guest_problems(probe(ids=[SECOND], samples=["/run/tqa/state/" + SECOND + ".lock"]),
                                               FIRST, SECOND), [])

    def test_every_leak_is_named(self):
        problems = checks.guest_problems(probe(
            state=["events.wal", FIRST + ".wal"], state_mounts=[["/dev/vdc", "/run/tqa/state"]] * 2,
            ids=[FIRST, SECOND], samples=["/workspace/.x/" + FIRST]), FIRST, SECOND)
        joined = "\n".join(problems)
        self.assertIn("2 devices are mounted at the state path", joined)
        self.assertIn("the state directory holds the first conversation's " + FIRST + ".wal", joined)
        self.assertIn("the second guest reaches a path naming the first conversation", joined)
        self.assertIn("/workspace/.x/" + FIRST, joined)

    def test_the_probe_record_fits_a_tool_result(self):
        self.assertLess(len(checks.PROBE), 4096)
        self.assertIn("[:20]", checks.PROBE)

    def test_a_probe_without_its_record_fails(self):
        self.assertIn("no probe record", checks.guest_problems("shell said nothing", FIRST, SECOND)[0])


class Host(unittest.TestCase):
    def mounts(self, state_source, extra=()):
        return [{"Destination": "/vm/state.img", "Source": state_source},
                {"Destination": "/vm/workspace.img", "Source": "/storage/workspaces/w.ext4"}] + list(extra)

    def test_the_second_vm_host_binds_its_own_state_image_only(self):
        self.assertEqual(checks.host_problems(self.mounts("/storage/runtime-state/" + SECOND + ".ext4"), FIRST,
                                              SECOND), [])

    def test_a_foreign_or_extra_state_image_is_named(self):
        problems = checks.host_problems(self.mounts("/storage/runtime-state/" + FIRST + ".ext4"), FIRST, SECOND)
        self.assertTrue(any("not the second conversation's" in problem for problem in problems), problems)
        leak = self.mounts("/storage/runtime-state/" + SECOND + ".ext4",
                           [{"Destination": "/vm/extra.img", "Source": "/storage/runtime-state/" + FIRST + ".ext4"}])
        self.assertTrue(any(FIRST in problem for problem in checks.host_problems(leak, FIRST, SECOND)))


class History(unittest.TestCase):
    def frame(self, seq, text):
        return {"seq": seq, "event": {"type": "text.delta", "text": text}}

    def test_the_first_history_only_grows_and_holds_nothing_forged(self):
        before = [self.frame(1, "a"), self.frame(2, "b")]
        self.assertEqual(checks.history_problems(before, before + [self.frame(3, "c")]), [])

    def test_the_marker_only_counts_after_before_and_never_in_a_tool_call(self):
        # G-13-history's false positive: the first conversation's own probe call carried the marker in its script.
        call = {"seq": 3, "event": {"type": "tool.call", "arguments": {"command": "x " + checks.FORGED}}}
        before = [self.frame(1, "a"), call]
        self.assertEqual(checks.history_problems(before, before + [self.frame(4, "c")]), [])
        later_call = {"seq": 5, "event": {"type": "tool.call", "arguments": {"command": checks.FORGED}}}
        self.assertEqual(checks.history_problems(before, before + [later_call]), [])

    def test_the_dump_names_every_marker_frame_including_tool_calls(self):
        call = {"seq": 3, "event": {"type": "tool.call", "arguments": {"command": checks.FORGED}}}
        leak = {"seq": 9, "event": {"type": "text.delta", "text": checks.FORGED}}
        dump = checks.marker_frames([self.frame(1, "a"), call, leak])
        self.assertEqual([(entry["seq"], entry["type"]) for entry in dump], [(3, "tool.call"), (9, "text.delta")])

    def test_a_changed_prefix_or_a_forged_frame_fails(self):
        before = [self.frame(1, "a"), self.frame(2, "b")]
        self.assertIn("rewritten", checks.history_problems(before, [self.frame(1, "a"), self.frame(2, "X")])[0])
        forged = before + [self.frame(3, checks.FORGED)]
        self.assertIn("forged", checks.history_problems(before, forged)[0])


if __name__ == "__main__":
    unittest.main()
