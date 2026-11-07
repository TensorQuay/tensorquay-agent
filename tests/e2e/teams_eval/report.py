"""The agent-teams evaluation after its runs (docs/evals/agent-teams.md section 2):

- `packets RUN [RUN ...] OUT`: the blind packets from the runs' final attempts (judging.make_packets), and one empty
  sheet per judge (first.csv, second.csv, third.csv); the side map stays beside them for whoever joins the scores back.
- `report RUN [RUN ...] OUT`: the judges' sheets settled (judging.settle), joined to the attempts, and the numbers
  (stats.analyse) written as report.md and report.json; packets still waiting for the third judge are listed, never
  guessed.

Each RUN is one batch of the night, the runner's own --out (`fri-d`, `fri-s`, `sat-l`, `sat-d`): its name is its
directory's, an attempt is known by its batch with its task, side and number (each batch numbers from #1), a rerun
replaces an attempt of its own batch only, and the numbers combine every batch's final attempts by task.

`packets --leave-out BATCH:SIDE` (repeatable) leaves one batch's side out of the judging and the numbers: night 1's
team side started no helper, so it is left out, and night 3's team side (`fri-d-team`, `fri-s-team`) is compared
with night 1's single attempts, task by task (the technical lead, 12 Oct 2026). The packets step keeps the choice in
`left-out.json`, and the report reads it from there and says it, so the two steps never disagree.

Pure: it reads a run's directory and the task fixtures, never a stack.
"""

import argparse
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import judging  # noqa: E402
import runner  # noqa: E402
import stats  # noqa: E402

JUDGES = ("first", "second", "third")
PACKET_SEED = 20261006


def load_lines(run):
    with open(Path(run) / "attempts.jsonl") as handle:
        return [json.loads(line) for line in handle if line.strip()]


def runs_of(run):
    """The runs given: one directory, or several (the night's batches)."""
    return [Path(run)] if isinstance(run, (str, Path)) else [Path(each) for each in run]


def load_batches(runs, leave_out=()):
    """Every run's attempt lines, each marked with its batch (the run's directory name) and its run, but those of a
    `(batch, side)` in `leave_out`. Two runs of one name would mix their attempts, so they are refused."""
    names = [run.name for run in runs]
    twice = sorted({name for name in names if names.count(name) > 1})
    if twice:
        raise ValueError(f"two runs share a batch name: {', '.join(twice)}; each batch needs its own --out")
    unknown = sorted(f"{batch}:{side}" for batch, side in leave_out if batch not in names or side not in runner.SIDES)
    if unknown:
        raise ValueError(f"--leave-out names no batch and side given: {', '.join(unknown)}")
    left = {tuple(each) for each in leave_out}
    return [dict(line, batch=run.name, run=str(run)) for run in runs for line in load_lines(run)
            if (run.name, line["side"]) not in left]


def left_out(out):
    """The (batch, side) pairs the packets step left out, from its `left-out.json` (none before it existed)."""
    path = Path(out) / "left-out.json"
    return [tuple(each) for each in json.loads(path.read_text())] if path.exists() else []


def final_of(lines):
    """Each batch's final attempts (runner.final within it: a rerun replaces its own batch's attempt only), in the
    order the batches were given."""
    batches = []
    for line in lines:
        if line.get("batch") not in batches:
            batches.append(line.get("batch"))
    return [line for batch in batches for line in runner.final([each for each in lines if each.get("batch") == batch])]


def directory_of(run, line):
    """An attempt's saved records: under its own run when the line names one."""
    return Path(line.get("run", run)) / "attempts" / f"{line['task']}-{line['side']}-{line['attempt']}"


def judging_records(run, lines, tasks):
    """One record per final attempt that ran, for judging.make_packets. The words redacted are the same for both sides
    of a task: every helper name and role any of its team attempts had, so the rule itself is symmetric and a single
    agent that writes "the analyst" gets "a colleague" too."""
    by_id = {task.id: task for task in tasks}
    words = {}
    final = final_of(lines)
    for line in final:
        words.setdefault(line["task"], set()).update(line.get("helper_names", []) + line.get("helper_roles", []))
    records = []
    for line in final:
        if not line.get("conversation"):
            continue  # never started: nothing to judge
        directory = directory_of(run, line)
        task = by_id[line["task"]]
        records.append({"task": line["task"], "side": line["side"], "attempt": line["attempt"],
                        "statement": task.text, "key_facts": list(task.key_facts),
                        "answer": (directory / "answer.md").read_text(),
                        "files": json.loads((directory / "changed.json").read_text()),
                        "helpers": sorted(words[line["task"]]), "batch": line.get("batch")})
    return records


def make_packets(run, out, tasks, leave_out=()):
    lines = load_batches(runs_of(run), leave_out)
    order = judging.make_packets(judging_records(None, lines, tasks), out, PACKET_SEED)
    (Path(out) / "left-out.json").write_text(json.dumps([list(each) for each in leave_out]) + "\n")
    for judge in JUDGES:
        judging.write_sheet(Path(out) / f"{judge}.csv", order)
    return order


# The read hints' sentences (crates/tqa-tools/src/builtin/team/tools.rs, `render`): the last line of a read_reports
# result, after its message blocks, so a helper's own words inside a block are never read as one.
NOTHING_NEW = "Nothing new yet."
YIELD_LINE = re.compile(r"Another task is waiting for a slot: .*|Helper .+ is waiting to work on your files: .*")
PLACED = re.compile(r"(?:^|, )(.+?) as (uploads/[^ ]+) \([0-9,]+ bytes\)")


def renamed(path, wanted):
    """Whether `path` is the gateway's next free name for `wanted` (`stem-N.ext`, N >= 2): the name was taken."""
    dot = wanted.find(".")
    stem, extension = (wanted, "") if dot < 0 else (wanted[:dot], wanted[dot:])
    found = re.fullmatch(re.escape(f"uploads/{stem}-") + r"([0-9]+)" + re.escape(extension), path)
    return found is not None and int(found.group(1)) >= 2


def team_measures(run, line):
    """(take_conflicts, waits, yields) of a team attempt, from its saved records (events.jsonl, team.json):
    - a take_files call refused, or a file it placed under another name than its own (`<helper>-<path>`), because one
      was there: take_files never overwrites, so a collision shows as the next free name;
    - waits: the lead's read_reports calls that waited (`wait` > 0; a lead's read without it doesn't) and found
      nothing new: a wait on its helpers' output. A read with wait 0 is a poll;
    - yields: its read_reports results that told it to end its turn, for another task waiting for a slot or for a
      helper waiting to work on its files: pressure on the lead, not a wait on a helper.
    (0, 0, 0) for a single attempt or one without records."""
    directory = directory_of(run, line)
    if line["side"] != "team" or not (directory / "events.jsonl").is_file():
        return 0, 0, 0
    frames = [json.loads(text) for text in (directory / "events.jsonl").read_text().splitlines() if text.strip()]
    team = json.loads((directory / "team.json").read_text()) if (directory / "team.json").is_file() else {}
    authors = {message["seq"]: message.get("from", {}).get("name") for message in team.get("messages") or []
               if message.get("type") == "report"}
    events = [frame["event"] for frame in frames if frame.get("type") == "event"]
    calls = {event.get("call_id"): event for event in events if event.get("type") == "tool.call"}
    conflicts = waits = yields = 0
    for event in events:
        call = calls.get(event.get("call_id")) if event.get("type") == "tool.result" else None
        if call is None:
            continue
        content = event.get("content") or ""
        if call.get("name") == "take_files":
            if event.get("status") == "error":
                conflicts += 1
                continue
            member = authors.get((call.get("arguments") or {}).get("report_seq"))
            for name, path in PLACED.findall(content.split(": ", 1)[-1]):
                conflicts += member is not None and renamed(path, f"{member}-{name.replace('/', '-')}")
        elif call.get("name") == "read_reports" and event.get("status") != "error":
            last = content.rstrip().rsplit("\n", 1)[-1]
            waited = ((call.get("arguments") or {}).get("wait") or 0) > 0
            waits += last == NOTHING_NEW and waited
            yields += YIELD_LINE.fullmatch(last) is not None
    return conflicts, waits, yields


def attempts(lines, scores, run=None):
    """stats.Attempt for each final attempt, its judged score from `scores` ({(batch, task, side, attempt): total}),
    and with its run (`run`, or the run its line names), a team attempt's own measures from its records."""
    out = []
    for line in final_of(lines):
        measured = run is not None or line.get("run") is not None
        conflicts, waits, yields = team_measures(run, line) if measured else (0, 0, 0)
        key = (line.get("batch"), line["task"], line["side"], line["attempt"])
        out.append(stats.Attempt(line["task"], line["kind"], line["side"], line.get("passed"),
                                 scores.get(key), line.get("tokens", 0),
                                 line.get("wall_secs", 0.0), tuple(line.get("failures", ())), line.get("helpers", 0),
                                 conflicts, waits, yields))
    return out


def scope(lines):
    """The limits' first line, from what the run holds: its tasks by kind and its attempts per task and side."""
    final = final_of(lines)
    tasks = sorted({line["task"] for line in final})
    counts = {}
    for line in final:
        counts[(line["task"], line["side"])] = counts.get((line["task"], line["side"]), 0) + 1
    low, high = min(counts.values(), default=0), max(counts.values(), default=0)
    each = f"{low} attempt{'s' if low != 1 else ''}" if low == high else f"{low} to {high} attempts"
    named = f"{len(tasks)} task{'s' if len(tasks) != 1 else ''} ({', '.join(tasks)})"
    batches = batch_names(lines)
    over = f", over {len(batches)} batches ({', '.join(batches)})" if len(batches) > 1 else ""
    return f"One model route, {named}, {each} per task and side{over}, local sources only."


def batch_names(lines):
    """The batches the lines come from, in the order given (none for lines without one)."""
    names = []
    for line in lines:
        if line.get("batch") is not None and line["batch"] not in names:
            names.append(line["batch"])
    return names


def batches_line(lines):
    """Which batch holds what: each batch's tasks and final attempts, when there are several."""
    names = batch_names(lines)
    if len(names) < 2:
        return []
    final = final_of(lines)
    parts = []
    for name in names:
        mine = [line for line in final if line.get("batch") == name]
        tasks = sorted({line["task"] for line in mine})
        parts.append(f"{name} ({', '.join(tasks)}; {len(mine)} attempt{'s' if len(mine) != 1 else ''})")
    return [f"Batches, combined by task: {'; '.join(parts)}.", ""]


def read_sheets(out, order):
    sheets = {}
    for judge in JUDGES:
        path = Path(out) / f"{judge}.csv"
        sheets[judge] = judging.read_sheet(path, order) if path.is_file() else {}
    return sheets


def interval(pair, scale=1.0, unit=""):
    low, high = pair
    if low is None:
        return "–"
    return f"{low * scale:+.2f}{unit} to {high * scale:+.2f}{unit}"


def number(value, form="{:.2f}"):
    return "–" if value is None else form.format(value)


def load_switches(run):
    """The gateway restarts that switched sides, from every run given; each marked with its batch when there are
    several."""
    runs = runs_of(run)
    found = []
    for each in runs:
        path = each / "switches.jsonl"
        if not path.is_file():
            continue
        for line in path.read_text().splitlines():
            if line.strip():
                entry = json.loads(line)
                if len(runs) > 1:
                    entry["batch"] = each.name
                found.append(entry)
    return found


LIMITS = (
    "Redaction replaces helper names, roles and side words with \"a colleague\" on both sides, but those words "
    "only ever appear in team answers, so \"a colleague\" still tends to mark a team answer: a residual tell, "
    "measured by the judges' side guesses above.",
)


def blindness(out, side_map):
    """Each judge's accuracy at guessing the side, from the side_guess column of their sheet."""
    found = {}
    for judge in JUDGES:
        path = Path(out) / f"{judge}.csv"
        if path.is_file():
            found[judge] = judging.guess_accuracy(judging.read_guesses(path), side_map)
    return found


def markdown(found, lines, waiting, switches=(), incomplete=(), guesses=None, leave_out=()):
    sides = found["sides"]
    out = ["# Agent teams evaluation: results", ""]
    if leave_out:
        out += ["**Left out:** " + ", ".join(f"{batch}'s {side} side" for batch, side in leave_out)
                + " (not judged, not counted).", ""]
    if waiting:
        out += [f"**Not final:** {len(waiting)} packet(s) wait for the third judge: {', '.join(waiting)}.", ""]
    if incomplete:
        out += [f"**Not final:** {len(incomplete)} packet(s) are not yet fully scored by both judges: "
                f"{', '.join(incomplete)}.", ""]
    out += batches_line(lines)
    out += ["| | Single agent | Agent team |", "|---|---|---|"]
    for label, key, form in (("Attempts", "attempts", "{}"), ("Passed (checked)", None, None),
                             ("Mean judged score (0-10)", "mean_score", "{:.2f}"), ("Tokens", "tokens", "{:,}"),
                             ("Wall time (min)", "wall_secs", None)):
        if key is None and label.startswith("Passed"):
            cells = [f"{sides[s]['passed']}/{sides[s]['checked']} (95 % {interval(sides[s]['pass_interval'], 100)} %)"
                     .replace("+", "") for s in stats.SIDES]
        elif key == "wall_secs":
            cells = [f"{sides[s]['wall_secs'] / 60:.0f}" for s in stats.SIDES]
        else:
            cells = [number(sides[s][key], form) for s in stats.SIDES]
        out.append(f"| {label} | {cells[0]} | {cells[1]} |")
    out += ["", "Paired differences, team minus single, per task; 95 % paired bootstrap over tasks:", "",
            "| Tasks | Score: mean (interval) | Pass rate: mean (interval) |", "|---|---|---|"]
    for label, block in [("All", found["overall"]), ("Split work (research, report)", found["split"]),
                         ("Short (research, report, code, mixed)", found["by_group"]["short"]["comparison"])] + [
            (kind, found["by_kind"][kind]) for kind in stats.KINDS]:
        score, rate = block["score"], block["pass_rate"]
        out.append(f"| {label} ({score['tasks']}) | {number(score['mean'], '{:+.2f}')} ({interval(score['interval'])})"
                   f" | {number(rate['mean'], '{:+.2f}')} ({interval(rate['interval'])}) |")
    out += groups_markdown(found)
    out += ["", "| Task | Single: passed, scores | Team: passed, scores, helpers |", "|---|---|---|"]
    final = final_of(lines)
    for task in sorted({line["task"] for line in final}):
        cells = []
        for side in stats.SIDES:
            mine = [line for line in final if line["task"] == task and line["side"] == side]
            passed = sum(1 for line in mine if line.get("passed"))
            checked = sum(1 for line in mine if line.get("passed") is not None)
            cell = f"{passed}/{checked}"
            if side == "team":
                cell += ", helpers " + " ".join(str(line.get("helpers", 0)) for line in mine)
            cells.append(cell)
        out.append(f"| {task} | {cells[0]} | {cells[1]} |")
    out += ["", "Failure classes (attempts showing each):", ""]
    for side in stats.SIDES:
        counts = sides[side]["failures"]
        out.append(f"- {side}: " + (", ".join(f"{name} {count}" for name, count in counts.items()) or "none"))
    out.append(f"- leads that worked alone: {number(found['lead_alone_share'], '{:.0%}')} of team attempts")
    reruns = [line for line in lines if line.get("rerun_of")]
    several = len(batch_names(lines)) > 1
    out.append(f"- reruns: {len(reruns)}" + (" (" + ", ".join(
        (f"{line['batch']} " if several else "")
        + f"{line['task']} {line['side']} #{line['rerun_of']}: {line.get('rerun') or 'rerun'}" for line in reruns) + ")"
        if reruns else ""))
    harness = [line for line in final_of(lines) if line.get("passed") is None and line.get("problem")]
    out.append(f"- not counted (the harness's): {len(harness)}")
    if switches:
        out += ["", f"Gateway restarts to switch sides ({len(switches)}, the lead's request of 5 Oct 2026):", ""]
        if any("batch" in entry for entry in switches):
            out += ["| Batch | At (UTC) | Before | To | Seconds |", "|---|---|---|---|---|"]
            out += [f"| {entry.get('batch', '')} | {entry['at']} | {entry['before']} | {entry['side']} | "
                    f"{entry['secs']} |" for entry in switches]
        else:
            out += ["| At (UTC) | Before | To | Seconds |", "|---|---|---|---|"]
            out += [f"| {entry['at']} | {entry['before']} | {entry['side']} | {entry['secs']} |" for entry in switches]
    if guesses:
        out += ["", "Blindness: each judge's guesses at the side (chance is 50 %; an interval wholly above it means "
                "the packets told that judge the side):", "",
                "| Judge | Guessed | Right | Share right (95 %) | Unsure |", "|---|---|---|---|---|"]
        for judge, accuracy in guesses.items():
            share = "–" if accuracy["share"] is None else (
                f"{accuracy['share']:.0%} ({interval(accuracy['interval'], 100).replace('+', '')} %)")
            out.append(f"| {judge} | {accuracy['guessed']} | {accuracy['right']} | {share} | {accuracy['unsure']} |")
    verdict = found["bars"]
    out += ["", f"**Against the proposed bars: {verdict['verdict']}.**"]
    out += [f"- {reason}" for reason in verdict["reasons"]]
    out += ["", "Limits:", ""] + [f"- {limit}" for limit in (scope(lines),) + LIMITS]
    return "\n".join(out) + "\n"


def groups_markdown(found):
    """Short, dependent and long on their own: each side's pass rate with its interval, mean score, medians per
    attempt, the paired cost ratios, and the team's own measures."""
    out = ["", "By group (short: research, report, code, mixed); medians per attempt; ratios team over single, paired "
           "per task:", "",
           "| Group | Side | Passed (95 %) | Mean score | Median tokens | Median min | Median min, passed |",
           "|---|---|---|---|---|---|---|"]
    for name, block in found["by_group"].items():
        for side in stats.SIDES:
            summary = block["sides"][side]
            if not summary["attempts"]:
                continue
            minutes = [number(None if summary[key] is None else summary[key] / 60, "{:.0f}")
                       for key in ("median_wall_secs", "median_wall_secs_passed")]
            passed = (f"{summary['passed']}/{summary['checked']} "
                      f"({interval(summary['pass_interval'], 100).replace('+', '')} %)")
            out.append(f"| {name} | {side} | {passed} | {number(summary['mean_score'])} | "
                       f"{number(summary['median_tokens'], '{:,.0f}')} | {minutes[0]} | {minutes[1]} |")
    out += ["", "| Group | Tasks paired | Tokens ratio | Wall time ratio | Team: helpers (mean) | Hand-backs not as "
            "handed | Reads that waited and found nothing | Reads told to yield |", "|---|---|---|---|---|---|---|---|"]
    for name, block in found["by_group"].items():
        team = block["sides"]["team"]
        if not team["attempts"]:
            continue
        out.append(f"| {name} | {block['comparison']['score']['tasks']} | {number(block['token_ratio'], '{:.2f}x')} | "
                   f"{number(block['wall_ratio'], '{:.2f}x')} | {number(team['mean_helpers'], '{:.1f}')} | "
                   f"{team['take_conflicts']} | {team['waits']} | {team['yields']} |")
    return out


def make_report(run, out):
    runs = runs_of(run)
    leave_out = left_out(out)
    lines = load_batches(runs, leave_out)
    order = json.loads((Path(out) / "order.json").read_text())
    sheets = read_sheets(out, order)
    totals, waiting, incomplete = judging.settle(sheets["first"], sheets["second"], sheets["third"], order)
    side_map = json.loads((Path(out) / "side-map.json").read_text())
    # A side map made before batches names none: it is the one run's.
    found = stats.analyse(attempts(lines, judging.join(totals, side_map, runs[0].name if len(runs) == 1 else None)))
    guesses = blindness(out, side_map)
    (Path(out) / "report.json").write_text(json.dumps({"analysis": found, "waiting": waiting,
                                                      "incomplete": incomplete, "blindness": guesses}, indent=1,
                                                      default=list) + "\n")
    text = markdown(found, lines, waiting, load_switches(run), incomplete, guesses, leave_out)
    (Path(out) / "report.md").write_text(text)
    return text, waiting


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("step", choices=("packets", "report"))
    parser.add_argument("runs", type=Path, nargs="+", help="the runner's --out, one per batch of the night")
    parser.add_argument("out", type=Path, help="the judging directory")
    parser.add_argument("--tasks", type=Path, default=Path(__file__).resolve().parent / "tasks")
    parser.add_argument("--leave-out", action="append", default=[], metavar="BATCH:SIDE",
                        help="packets only: leave this batch's side out of the judging and the numbers (repeatable)")
    args = parser.parse_args(argv)
    leave_out = [tuple(each.split(":", 1)) for each in args.leave_out]
    if any(len(each) != 2 for each in leave_out):
        raise SystemExit("--leave-out is BATCH:SIDE, e.g. fri-d:team")
    if leave_out and args.step != "packets":
        raise SystemExit("--leave-out goes with packets; the report reads it from the judging directory")
    if args.step == "packets":
        if args.out.exists():
            raise SystemExit(f"{args.out} exists; packets are never overwritten")
        order = make_packets(args.runs, args.out, runner.select_tasks(args.tasks), leave_out)
        print(f"{len(order)} packets in {args.out / 'packets'}; sheets: " + ", ".join(f"{j}.csv" for j in JUDGES))
    else:
        text, waiting = make_report(args.runs, args.out)
        print(text)
        if waiting:
            print(f"{len(waiting)} packet(s) wait for the third judge", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
