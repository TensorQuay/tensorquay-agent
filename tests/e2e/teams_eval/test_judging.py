"""The agent-teams evaluation's blind judging without a run: packets that name no side, the sheets, the third judge on
a split, and the join back to attempts.

Run: python3 -m unittest tests/e2e/teams_eval/test_judging.py"""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import judging  # noqa: E402


def record(side, attempt, answer="Five causes.", files=None, helpers=()):
    return {"task": "R1", "side": side, "attempt": attempt, "statement": "Find the five causes.\n",
            "key_facts": ["disk full", "expired certificate"], "answer": answer, "files": files or {},
            "helpers": list(helpers)}


class Redaction(unittest.TestCase):
    def test_side_words_and_helper_names_are_redacted(self):
        text = "My helpers (analyst and fixer) split it; the agent team agreed. Analyst found the disk issue."
        out = judging.redact(text, ["analyst", "fixer"])
        for word in ("helper", "analyst", "Analyst", "fixer", "agent team"):
            self.assertNotIn(word, out)
        self.assertIn("found the disk issue", out)
        self.assertEqual(out, "Colleagues (a colleague and a colleague) split it; colleagues agreed. "
                              "A colleague found the disk issue.", "a neutral noun, with its article, no mark")

    def test_a_taken_files_path_names_no_helper(self):
        self.assertEqual(judging.redact_path("uploads/analyst-comparison.md", ["analyst"]),
                         "uploads/part-comparison.md")
        self.assertEqual(judging.redact_path("src/analysis.py", ["analyst"]), "src/analysis.py",
                         "only whole names are replaced")


class Symmetry(unittest.TestCase):
    def test_a_single_agents_answer_gets_the_same_substitution(self):
        self.assertEqual(judging.redact("I asked a helper. The analyst in me agrees.", ["analyst"]),
                         "I asked a colleague. A colleague in me agrees.")


class Packets(unittest.TestCase):
    def test_packets_hold_the_work_and_never_the_side(self):
        with tempfile.TemporaryDirectory() as root:
            records = [record("single", 1), record("team", 1, answer="My helper analyst wrote it.",
                                                   files={"uploads/analyst-causes.md": "analyst: disk full"},
                                                   helpers=["analyst"])]
            order = judging.make_packets(records, root, seed=7)
            self.assertEqual(len(order), 2)
            side_map = json.loads((Path(root) / "side-map.json").read_text())
            self.assertEqual(sorted(entry["side"] for entry in side_map.values()), ["single", "team"])
            for packet in order:
                directory = Path(root) / "packets" / packet
                everything = " ".join(path.read_text() for path in directory.rglob("*") if path.is_file())
                everything += " ".join(str(path) for path in directory.rglob("*"))
                for word in ("single", "team", "analyst", "helper"):
                    self.assertNotIn(word, everything.replace("task.md", ""), packet)
                self.assertEqual((directory / "key-facts.md").read_text(), "- disk full\n- expired certificate\n")
            self.assertEqual(judging.make_packets(records, Path(root) / "again", seed=7), order, "seeded")

    def test_a_path_leaving_the_packet_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            for path in ("../outside.md", "/tmp/outside.md"):
                with self.assertRaises(ValueError, msg=path):
                    judging.make_packets([record("single", 1, files={path: "x"})], root, seed=1)

    def test_roles_are_redacted_and_taken_files_lose_their_layout(self):
        with tempfile.TemporaryDirectory() as root:
            files = {"uploads/scout-causes.md": "scout: disk full", "uploads/scout-notes.md": "a",
                     "notes.md": "the lead's own"}
            records = [record("team", 1, answer="The researcher found it; according to scout, disk.", files=files,
                              helpers=["scout", "researcher"])]
            packet = judging.make_packets(records, root, seed=3)[0]
            directory = Path(root) / "packets" / packet
            placed = sorted(str(path.relative_to(directory / "files")) for path in (directory / "files").rglob("*")
                            if path.is_file())
            self.assertEqual(placed, ["causes.md", "notes.md", "part-notes.md"])
            answer = (directory / "answer.md").read_text()
            self.assertNotIn("researcher", answer)
            self.assertNotIn("scout", answer)


class Sheets(unittest.TestCase):
    def sheet(self, root, name, rows):
        path = Path(root) / name
        judging.write_sheet(path, [packet for packet, _ in rows])
        lines = path.read_text().splitlines()
        body = [lines[0]] + [",".join([packet] + [str(score) for score in scores] + [""]) for packet, scores in rows]
        path.write_text("\n".join(body) + "\n")
        return judging.read_sheet(path)

    def test_agreeing_judges_settle_at_the_mean_and_a_split_waits_for_the_third(self):
        with tempfile.TemporaryDirectory() as root:
            first = self.sheet(root, "a.csv", [("p1", (2, 2, 1, 2, 1)), ("p2", (0, 1, 1, 1, 1)),
                                               ("p3", (2, 2, 2, 2, 2))])
            second = self.sheet(root, "b.csv", [("p1", (1, 2, 2, 2, 1)), ("p2", (2, 1, 1, 1, 1)),
                                                ("p3", (1, 1, 1, 2, 2))])
            # p1 agrees (8 and 8); p2 has a criterion 0 against 2; p3 has no such criterion but totals 10 against 7.
            totals, waiting, _ = judging.settle(first, second)
            self.assertEqual(totals, {"p1": 8.0})
            self.assertEqual(waiting, ["p2", "p3"])
            third = {"p2": dict(zip(judging.CRITERIA, (1, 1, 1, 1, 1))),
                     "p3": dict(zip(judging.CRITERIA, (2, 2, 1, 2, 2)))}
            totals, waiting, _ = judging.settle(first, second, third)
            self.assertEqual(totals, {"p1": 8.0, "p2": 5.0, "p3": 9.0})
            self.assertEqual(waiting, [])

    def test_totals_two_points_apart_need_no_third(self):
        a = dict(zip(judging.CRITERIA, (2, 2, 2, 1, 1)))
        b = dict(zip(judging.CRITERIA, (1, 1, 2, 1, 1)))
        self.assertFalse(judging.split(a, b))
        self.assertEqual(judging.settle({"p": a}, {"p": b}), ({"p": 7.0}, [], []))

    def test_a_third_who_scored_only_part_leaves_the_packet_waiting(self):
        a = dict(zip(judging.CRITERIA, (0, 1, 1, 1, 1)))
        b = dict(zip(judging.CRITERIA, (2, 1, 1, 1, 1)))
        self.assertEqual(judging.settle({"p": a}, {"p": b}, {"p": {"correct": 1}}), ({}, ["p"], []))

    def test_a_score_outside_the_rubric_is_refused_naming_its_cell(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaisesRegex(ValueError, "p1, correct: '3'"):
                self.sheet(root, "a.csv", [("p1", (3, 2, 2, 2, 2))])

    def test_a_partly_scored_packet_has_no_total_yet(self):
        settled = judging.settle({"p1": {"correct": 2}}, {"p1": {"correct": 2}}, order=["p1", "p2"])
        self.assertEqual(settled, ({}, [], ["p1", "p2"]), "listed, never dropped: p2 has no row at all")

    def test_a_packet_scored_twice_or_unknown_is_refused_naming_its_row(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "a.csv"
            header = "packet," + ",".join(judging.CRITERIA) + ",notes\n"
            path.write_text(header + "p1,1,1,1,1,1,\np1,2,2,2,2,2,\n")
            with self.assertRaisesRegex(ValueError, "row 3: packet p1 is scored twice"):
                judging.read_sheet(path)
            path.write_text(header + "px,1,1,1,1,1,\n")
            with self.assertRaisesRegex(ValueError, "row 2: px is not one of the packets"):
                judging.read_sheet(path, order=["p1"])

    def test_scores_join_back_to_their_attempts(self):
        side_map = {"p1": {"task": "R1", "side": "team", "attempt": 2}, "p2": {"task": "R1", "side": "single",
                                                                               "attempt": 1}}
        self.assertEqual(judging.join({"p1": 7.5}, side_map), {(None, "R1", "team", 2): 7.5})


if __name__ == "__main__":
    unittest.main()
