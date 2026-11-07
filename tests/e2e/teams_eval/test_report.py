"""The agent-teams evaluation from a finished run to its report, without a stack: a synthetic run directory goes to
blind packets, filled sheets, and the report with its intervals, waiting packets and restarts.

Run: python3 -m unittest tests/e2e/teams_eval/test_report.py"""

import csv
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))

import report  # noqa: E402
import runner  # noqa: E402


def write_run(root):
    run = Path(root) / "run"
    lines = []
    for side in runner.SIDES:
        for number in (1, 2):
            directory = run / "attempts" / f"R1-{side}-{number}"
            directory.mkdir(parents=True)
            text = "My helper analyst found disk full." if side == "team" else "Disk full."
            (directory / "answer.md").write_text(text)
            (directory / "changed.json").write_text(json.dumps({"causes.md": "disk full\n"}))
            lines.append({"task": "R1", "kind": "research", "side": side, "attempt": number, "conversation": "c",
                          "passed": True, "tokens": 100, "wall_secs": 60.0, "failures": [],
                          "helpers": 1 if side == "team" else 0,
                          "helper_names": ["analyst"] if side == "team" else []})
    (run / "attempts.jsonl").write_text("".join(json.dumps(line) + "\n" for line in lines))
    (run / "switches.jsonl").write_text(json.dumps({"before": "R1", "side": "team", "at": "2026-10-07T09:00:00Z",
                                                    "secs": 8.5}) + "\n")
    return run


def fill(path, scores, guess=None):
    with open(path, newline="") as handle:
        rows = list(csv.reader(handle))
    for row in rows[1:]:
        row[1:6] = [str(score) for score in scores(row[0])]
        if guess is not None:
            row[6] = guess(row[0])
    with open(path, "w", newline="") as handle:
        csv.writer(handle).writerows(rows)


class FromRunToReport(unittest.TestCase):
    def test_a_packet_one_judge_left_blank_is_named_not_dropped(self):
        with tempfile.TemporaryDirectory() as root:
            run = write_run(root)
            task = runner.Task("R1", "research", "Find the causes.\n", 5, 60, 100, None, ("disk full",), Path(root))
            out = Path(root) / "judging"
            order = report.make_packets(run, out, [task])
            fill(out / "first.csv", lambda packet: (1, 1, 1, 1, 1))
            fill(out / "second.csv", lambda packet: ("",) * 5 if packet == order[0] else (1, 1, 1, 1, 1))
            text, _ = report.make_report(run, out)
            self.assertIn(f"1 packet(s) are not yet fully scored by both judges: {order[0]}", text)

    def test_the_report_measures_each_judges_guesses_and_names_the_residual_tell(self):
        with tempfile.TemporaryDirectory() as root:
            run = write_run(root)
            task = runner.Task("R1", "research", "Find the causes.\n", 5, 60, 100, None, ("disk full",), Path(root))
            out = Path(root) / "judging"
            order = report.make_packets(run, out, [task])
            side_map = json.loads((out / "side-map.json").read_text())
            # The first judge guesses every packet right; the second guesses "team" for all but one, left unsure.
            fill(out / "first.csv", lambda packet: (1, 1, 1, 1, 1), guess=lambda packet: side_map[packet]["side"])
            fill(out / "second.csv", lambda packet: (1, 1, 1, 1, 1),
                 guess=lambda packet: "" if packet == order[0] else "team")
            text, _ = report.make_report(run, out)
            self.assertIn("| first | 4 | 4 | 100%", text)
            right = sum(1 for packet in order[1:] if side_map[packet]["side"] == "team")
            self.assertIn(f"| second | 3 | {right} |", text)
            self.assertIn("a residual tell", text)

    def test_both_sides_of_a_task_get_its_helpers_names_redacted(self):
        with tempfile.TemporaryDirectory() as root:
            run = write_run(root)
            (run / "attempts" / "R1-single-1" / "answer.md").write_text("As an analyst would, I found disk full.")
            task = runner.Task("R1", "research", "Find the causes.\n", 5, 60, 100, None, ("disk full",), Path(root))
            out = Path(root) / "judging"
            report.make_packets(run, out, [task])
            side_map = json.loads((out / "side-map.json").read_text())
            single = next(packet for packet, entry in side_map.items()
                          if entry["side"] == "single" and entry["attempt"] == 1)
            self.assertEqual((out / "packets" / single / "answer.md").read_text(),
                             "As a colleague would, I found disk full.")

    def test_a_guess_outside_the_two_sides_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            run = write_run(root)
            task = runner.Task("R1", "research", "Find the causes.\n", 5, 60, 100, None, ("disk full",), Path(root))
            out = Path(root) / "judging"
            report.make_packets(run, out, [task])
            fill(out / "first.csv", lambda packet: (1, 1, 1, 1, 1), guess=lambda packet: "maybe")
            with self.assertRaisesRegex(ValueError, "side_guess 'maybe' is not single, team or blank"):
                report.make_report(run, out)

    def test_packets_sheets_and_report(self):
        with tempfile.TemporaryDirectory() as root:
            run = write_run(root)
            task = runner.Task("R1", "research", "Find the causes.\n", 5, 60, 100, None, ("disk full",), Path(root))
            out = Path(root) / "judging"
            order = report.make_packets(run, out, [task])
            self.assertEqual(len(order), 4)
            answers = " ".join((out / "packets" / packet / "answer.md").read_text() for packet in order)
            self.assertNotIn("analyst", answers)
            side_map = json.loads((out / "side-map.json").read_text())
            team = {packet for packet, entry in side_map.items() if entry["side"] == "team"}
            fill(out / "first.csv", lambda packet: (2, 2, 2, 2, 1) if packet in team else (1, 1, 1, 1, 1))
            # One team packet splits (0 against 2 on correct) and waits for the third judge.
            split = sorted(team)[0]
            fill(out / "second.csv", lambda packet: (0, 2, 2, 2, 1) if packet == split else
                 ((2, 2, 2, 2, 1) if packet in team else (1, 1, 1, 1, 1)))
            text, waiting = report.make_report(run, out)
            self.assertEqual(waiting, [split])
            self.assertNotIn("not yet fully scored", text)
            self.assertIn("**Not final:** 1 packet(s)", text)
            self.assertIn("| 2026-10-07T09:00:00Z | R1 | team | 8.5 |", text)
            fill(out / "third.csv", lambda packet: (2, 2, 2, 2, 1) if packet == split else ("",) * 5)
            text, waiting = report.make_report(run, out)
            self.assertEqual(waiting, [])
            found = json.loads((out / "report.json").read_text())["analysis"]
            self.assertEqual(found["overall"]["score"]["per_task"], {"R1": 4.0})
            self.assertIn("| R1 | 2/2 | 2/2, helpers 1 1 |", text)
            self.assertIn("| short | team | 2/2 (", text)
            self.assertIn("| short | 1 | 1.00x | 1.00x | 1.0 | 0 | 0 | 0 |", text)
            self.assertIn("| Short (research, report, code, mixed) (1) |", text)
            self.assertIn("- One model route, 1 task (R1), 2 attempts per task and side, local sources only.", text)
            self.assertNotIn("20 tasks", text)


def write_batch(root, name, task, kind, rerun=False, switched=False):
    """One batch of the night as the runner writes its --out (`<root>/<name>`): attempt #1 on each side, numbered from 1
    whatever the other batches hold; with `rerun`, the team's #1 rerun as #2 (the harness's)."""
    run = Path(root) / name
    lines = []
    attempts = [(side, 1, None) for side in runner.SIDES] + ([("team", 2, 1)] if rerun else [])
    for side, number, rerun_of in attempts:
        directory = run / "attempts" / f"{task}-{side}-{number}"
        directory.mkdir(parents=True)
        (directory / "answer.md").write_text(f"{name} {side} #{number}: done.")
        (directory / "changed.json").write_text(json.dumps({}))
        line = {"task": task, "kind": kind, "side": side, "attempt": number, "conversation": "c", "passed": True,
                "tokens": 100, "wall_secs": 60.0, "failures": [], "helpers": 1 if side == "team" else 0,
                "helper_names": ["writer"] if side == "team" else []}
        if rerun_of:
            line.update(rerun_of=rerun_of, rerun="harness")
        lines.append(line)
    (run / "attempts.jsonl").write_text("".join(json.dumps(line) + "\n" for line in lines))
    if switched:
        (run / "switches.jsonl").write_text(json.dumps({"before": task, "side": "team", "at": "2026-10-10T01:00:00Z",
                                                        "secs": 9.0}) + "\n")
    return run


class Batches(unittest.TestCase):
    """The night's batches (fri-d, fri-s, sat-l, sat-d), each its own --out: Saturday's second D attempts are
    numbered from #1 again, so an attempt is known by its batch, a rerun replaces its own batch's attempt, and the
    report combines every batch's final attempts by task."""

    def setUp(self):
        self.root = tempfile.TemporaryDirectory()
        self.addCleanup(self.root.cleanup)
        root = Path(self.root.name) / "eval-night"
        self.runs = [write_batch(root, "fri-d", "D1", "dependent"), write_batch(root, "fri-s", "R1", "research"),
                     write_batch(root, "sat-d", "D1", "dependent", rerun=True, switched=True)]
        self.tasks = [runner.Task(name, kind, "Do it.\n", 5, 60, 100, None, ("done",), Path(self.root.name))
                      for name, kind in (("D1", "dependent"), ("R1", "research"))]
        self.out = Path(self.root.name) / "judging"

    def test_packets_cover_every_batchs_final_attempts_and_the_side_map_names_the_batch(self):
        order = report.make_packets(self.runs, self.out, self.tasks)
        side_map = json.loads((self.out / "side-map.json").read_text())
        self.assertEqual(len(order), 6)
        found = sorted((entry["batch"], entry["task"], entry["side"], entry["attempt"]) for entry in side_map.values())
        self.assertEqual(found, [("fri-d", "D1", "single", 1), ("fri-d", "D1", "team", 1),
                                 ("fri-s", "R1", "single", 1), ("fri-s", "R1", "team", 1),
                                 ("sat-d", "D1", "single", 1), ("sat-d", "D1", "team", 2)],
                         "sat-d's rerun replaces its own team #1, never fri-d's")

    def test_scores_join_by_batch_so_two_attempts_numbered_1_stay_apart(self):
        lines = report.load_batches(self.runs)
        scores = {("fri-d", "D1", "team", 1): 7.0, ("sat-d", "D1", "team", 2): 3.0, ("sat-d", "D1", "team", 1): 99.0}
        team = sorted(attempt.score for attempt in report.attempts(lines, scores)
                      if attempt.task == "D1" and attempt.side == "team")
        self.assertEqual(team, [3.0, 7.0])

    def test_the_report_combines_the_batches_by_task_and_says_which_holds_what(self):
        order = report.make_packets(self.runs, self.out, self.tasks)
        for judge in ("first", "second"):
            fill(self.out / f"{judge}.csv", lambda packet: (1, 1, 1, 1, 1))
        text, waiting = report.make_report(self.runs, self.out)
        self.assertEqual(waiting, [])
        self.assertIn("Batches, combined by task: fri-d (D1; 2 attempts); fri-s (R1; 2 attempts); "
                      "sat-d (D1; 2 attempts).", text)
        self.assertIn("| D1 | 2/2 | 2/2, helpers 1 1 |", text)
        self.assertIn("| R1 | 1/1 | 1/1, helpers 1 |", text)
        self.assertIn("- reruns: 1 (sat-d D1 team #1: harness)", text)
        self.assertIn("| sat-d | 2026-10-10T01:00:00Z | D1 | team | 9.0 |", text)
        self.assertIn("1 to 2 attempts per task and side, over 3 batches (fri-d, fri-s, sat-d)", text)
        found = json.loads((self.out / "report.json").read_text())["analysis"]
        self.assertEqual(found["sides"]["team"]["attempts"], 3)
        self.assertEqual(len(order), 6)

    def test_two_runs_with_one_name_are_refused(self):
        other = write_batch(Path(self.root.name) / "elsewhere", "fri-d", "D1", "dependent")
        with self.assertRaisesRegex(ValueError, "two runs share a batch name: fri-d"):
            report.make_packets([self.runs[0], other], self.out, self.tasks)

    def test_a_side_map_made_before_batches_still_joins_for_its_one_run(self):
        run = self.runs[1]
        report.make_packets(run, self.out, self.tasks)
        side_map = json.loads((self.out / "side-map.json").read_text())
        for entry in side_map.values():
            entry.pop("batch")
        (self.out / "side-map.json").write_text(json.dumps(side_map))
        for judge in ("first", "second"):
            fill(self.out / f"{judge}.csv", lambda packet: (2, 2, 2, 2, 1))
        report.make_report(run, self.out)
        found = json.loads((self.out / "report.json").read_text())["analysis"]
        self.assertEqual(found["sides"]["single"]["mean_score"], 9.0)

    def test_a_batchs_side_left_out_is_neither_judged_nor_counted_and_the_report_says_so(self):
        # Night 1's team side started no helper: left out, the team compared with the other batches' team attempts.
        order = report.make_packets(self.runs, self.out, self.tasks, [("fri-d", "team"), ("fri-s", "team")])
        side_map = json.loads((self.out / "side-map.json").read_text())
        found = sorted((entry["batch"], entry["side"]) for entry in side_map.values())
        self.assertEqual(found, [("fri-d", "single"), ("fri-s", "single"), ("sat-d", "single"), ("sat-d", "team")])
        self.assertEqual(len(order), 4)
        for judge in ("first", "second"):
            fill(self.out / f"{judge}.csv", lambda packet: (1, 1, 1, 1, 1))
        text, _ = report.make_report(self.runs, self.out)
        self.assertIn("**Left out:** fri-d's team side, fri-s's team side (not judged, not counted).", text)
        found = json.loads((self.out / "report.json").read_text())["analysis"]
        self.assertEqual((found["sides"]["single"]["attempts"], found["sides"]["team"]["attempts"]), (3, 1))

    def test_a_left_out_side_must_name_a_batch_given_and_goes_with_packets_only(self):
        with self.assertRaisesRegex(ValueError, "names no batch and side given: sat-l:team"):
            report.make_packets(self.runs, self.out, self.tasks, [("sat-l", "team")])
        with self.assertRaisesRegex(SystemExit, "BATCH:SIDE"):
            report.main(["packets", *map(str, self.runs), str(self.out), "--leave-out", "fri-d"])
        with self.assertRaisesRegex(SystemExit, "goes with packets"):
            report.main(["report", *map(str, self.runs), str(self.out), "--leave-out", "fri-d:team"])

    def test_the_command_line_takes_the_batches_before_the_judging_directory(self):
        with patch.object(runner, "select_tasks", return_value=self.tasks):
            self.assertEqual(report.main(["packets", *map(str, self.runs), str(self.out)]), 0)
        self.assertEqual(len(json.loads((self.out / "order.json").read_text())), 6)


def tool(call_id, name, arguments, content, status="ok"):
    return [{"type": "event", "event": {"type": "tool.call", "call_id": call_id, "name": name,
                                        "arguments": arguments}},
            {"type": "event", "event": {"type": "tool.result", "call_id": call_id, "status": status,
                                        "content": content}}]


def block(text, number):
    return (f'<untrusted-content source="team" type="report" seq="{number}" id="b{number}">\n{text}\n'
            f'</untrusted-content id="b{number}">')


class TeamMeasures(unittest.TestCase):
    def write(self, root, frames, side="team"):
        directory = Path(root) / "attempts" / f"D1-{side}-1"
        directory.mkdir(parents=True)
        (directory / "events.jsonl").write_text("".join(json.dumps(frame) + "\n" for frame in frames))
        messages = [{"type": "report", "seq": 4, "from": {"name": "writer"}},
                    {"type": "report", "seq": 7, "from": {"name": "reader"}}]
        (directory / "team.json").write_text(json.dumps({"team": {}, "messages": messages}))
        return {"task": "D1", "side": side, "attempt": 1}

    def test_a_renamed_or_refused_hand_back_counts_and_one_placed_as_its_own_name_does_not(self):
        frames = (tool("1", "take_files", {"report_seq": 4},
                       "Placed 2 files from report 4 in uploads/: log/format.md as uploads/writer-log-format.md "
                       "(2,104 bytes), main.py as uploads/writer-main-2.py (10 bytes). Move them where they belong "
                       "with your shell.")
                  + tool("2", "take_files", {"report_seq": 7}, "uploads_full: no free name", status="error")
                  + tool("3", "take_files", {"report_seq": 7},
                         "Placed 1 file from report 7 in uploads/: notes-2.md as uploads/reader-notes-2.md (5 bytes)."
                         " Move them where they belong with your shell."))
        with tempfile.TemporaryDirectory() as root:
            line = self.write(root, frames)
            self.assertEqual(report.team_measures(root, line), (2, 0, 0),
                             "writer-main-2.py was renamed and report 7's first take refused; notes-2.md is its name")

    def test_a_read_that_waited_and_found_nothing_is_a_wait_and_a_hint_to_end_the_turn_a_yield(self):
        frames = (tool("1", "read_reports", {"wait": 30}, "Nothing new yet.")
                  + tool("0", "read_reports", {"wait": 0}, "Nothing new yet.")
                  + tool("9", "read_reports", {}, "Nothing new yet.")
                  + tool("2", "read_reports", {}, block("done", 4) + "\nHelper writer is waiting to work on your "
                         "files: end your turn now so it can; you'll be woken by its report.")
                  + tool("3", "read_reports", {}, "Another task is waiting for a slot: end your turn now; you'll be "
                         "woken by the next message.")
                  + tool("4", "read_reports", {"wait": 30}, block("Nothing new yet.", 7))
                  + tool("5", "read_reports", {}, block("x", 8) + "\nMore messages are waiting: read again."))
        with tempfile.TemporaryDirectory() as root:
            self.assertEqual(report.team_measures(root, self.write(root, frames)), (0, 1, 2),
                             "one read waited 30 s for nothing; polls, a helper's words and a full read aren't waits")

    def test_a_single_attempt_or_one_without_records_measures_nothing(self):
        with tempfile.TemporaryDirectory() as root:
            line = self.write(root, tool("1", "read_reports", {}, "Nothing new yet."), side="single")
            self.assertEqual(report.team_measures(root, line), (0, 0, 0))
            self.assertEqual(report.team_measures(root, {"task": "D9", "side": "team", "attempt": 1}), (0, 0, 0))

    def test_the_limits_name_the_runs_own_tasks_and_attempts(self):
        lines = [{"task": task, "kind": "dependent", "side": side, "attempt": number}
                 for task, attempts in (("D1", 2), ("L1", 1)) for side in runner.SIDES
                 for number in range(1, attempts + 1)]
        self.assertEqual(report.scope(lines),
                         "One model route, 2 tasks (D1, L1), 1 to 2 attempts per task and side, local sources only.")


if __name__ == "__main__":
    unittest.main()
