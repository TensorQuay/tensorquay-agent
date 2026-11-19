"""E2E-AGENTS-041's verdicts (ADR-0034's rounds amendment; plan 18.1 item E): reads a rounds run's attempts, as
runner.py keeps them (team.json, events.jsonl, events-helper-<name>.jsonl), and says per attempt the gateway's side
and the model's choices. python3 rounds_report.py <run> [<run> ...]

The gateway's side, which must hold in every attempt:
- two round records, round 2 opened once (one round_started) and after the person's follow-up;
- each helper brought back keeps its task: its journal has a run before round 2 and one after it;
- round 2's budget is its own: a record with its own budget, and tokens_all_rounds the sum of both;
- the person's message between the rounds reached its helper (a message.user with its text in that helper's journal),
  before that helper's round-2 run when it was brought back;
- no start_member refused (the page's "Couldn't start a helper").
The model's choices, recorded: the helpers round 1 started, those round 2 brought back and those it added; whether it
did what the follow-up asked (the two named back, one new).
"""
import json
import sys
from datetime import datetime
from pathlib import Path


def when(stamp):
    """A timestamp as a datetime: the log's microseconds and the journal's nanoseconds alike."""
    head, _, rest = stamp.rstrip("Z").partition(".")
    return datetime.fromisoformat(f"{head}.{(rest + '000000')[:6]}")


def journal(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()] if path.is_file() else []


def kinds(frames):
    return [(frame.get("ts"), (frame.get("event") or {}).get("type"), frame.get("event") or {}) for frame in frames]


def bring_backs(frames, follow_up):
    """What the lead asked for in round 2, from its own start_member calls after the person's follow-up, and what came
    back. A call naming a helper round 1 started, or naming `files`, asks for that helper back (A3: `files` is optional,
    keep by default, and an earlier round's name always brings it back); any other starts a new one. `not_brought_back`
    is each helper asked back whose answer didn't say "Brought back": the gateway's failure, never the model's (found on
    19 Nov 2026, when "Started helper totals-2" answered a bring-back). `refused` lists every refused start as (round,
    role, code), round 1's included (a malformed call of the model's, such as a brief without its objective)."""
    after = next((index for index, (_, kind, event) in enumerate(frames)
                  if kind == "message.user" and event.get("text", "").strip() == (follow_up or "").strip()), None)
    calls, answers, refused = {}, {}, []
    for index, (_, kind, event) in enumerate(frames):
        if kind == "tool.call" and event.get("name") == "start_member":
            calls[event.get("call_id")] = (index, event.get("arguments") or {})
        elif kind == "tool.result" and event.get("call_id") in calls:
            answers[event.get("call_id")] = (str(event.get("content") or ""), event.get("status"))
    for call, (index, arguments) in calls.items():
        text, status = answers.get(call, ("", None))
        if status not in (None, "ok"):
            round_ = 2 if after is not None and index > after else 1
            refused.append((round_, (arguments.get("role") or "").lower(), text.split(":", 1)[0].strip()))
    if after is None:
        return {"back": [], "new": [], "not_brought_back": [], "refused": refused}
    first = {(arguments.get("role") or "").lower() for call, (index, arguments) in calls.items()
             if index < after and answers.get(call, ("", None))[1] == "ok"}
    second = {call: (arguments.get("role") or "").lower() for call, (index, arguments) in calls.items() if index > after}
    back = {call: role for call, role in second.items() if role in first or calls[call][1].get("files")}
    new = sorted({role for call, role in second.items() if call not in back})
    missed = sorted({role for call, role in back.items() if not answers.get(call, ("", None))[0].startswith("Brought back")})
    return {"back": sorted(set(back.values())), "new": new, "not_brought_back": missed, "refused": refused}

def attempt(directory, task_dir):
    record = json.loads((directory / "team.json").read_text())
    team, log = record["team"], record["messages"]
    follow_up = (task_dir / "follow_up.md").read_text().strip() if (task_dir / "follow_up.md").is_file() else None
    between = json.loads((task_dir / "between.json").read_text()) if (task_dir / "between.json").is_file() else None
    lead = kinds(journal(directory / "events.jsonl"))
    started = [message for message in log if message["type"] == "round_started"]
    opened_at = started[0]["at"] if started else None
    opened_seq = started[0]["seq"] if started else None
    asked_at = next((ts for ts, kind, event in lead if kind == "message.user" and follow_up
                     and event.get("text", "").strip() == follow_up), None)
    assigns = [message for message in log if message["type"] == "assign"]
    to = lambda message: (message.get("to") or {}).get("name")  # noqa: E731
    # By the log's order: round 2's start and its first assign share a transaction, so a time never parts them.
    first = sorted({to(message) for message in assigns if opened_seq is None or message["seq"] < opened_seq} - {None})
    second = sorted({to(message) for message in assigns if opened_seq and message["seq"] > opened_seq} - {None})
    back, new = sorted(set(second) & set(first)), sorted(set(second) - set(first))
    gateway, notes = [], []

    def need(ok, words):
        gateway.append((ok, words))

    need(len(team.get("rounds", [])) == 2 and team.get("round") == 2, "two round records, round 2 the team's")
    need(len(started) == 1, f"one round_started ({len(started)})")
    need(bool(asked_at and opened_at and when(opened_at) > when(asked_at)), "round 2 opened after the person's follow-up")
    rounds = team.get("rounds", [])
    if len(rounds) == 2:
        need(rounds[1]["budget"]["tokens"] > 0 and rounds[0]["ended_at"] is not None
             and when(rounds[1]["started_at"]) >= when(rounds[0]["ended_at"]),
             "round 2 has its own budget, started after round 1 ended")
        need(team.get("tokens_all_rounds") == sum(each["budget"]["spent"] for each in rounds),
             "tokens_all_rounds is both rounds' spend")
    for name in back:
        runs = [ts for ts, kind, _ in kinds(journal(directory / f"events-helper-{name}.jsonl")) if kind == "run.started"]
        need(bool(runs) and when(runs[0]) < when(opened_at) < when(runs[-1]),
             f"{name} brought back: its journal runs on into round 2")
    if between:
        helper = kinds(journal(directory / f"events-helper-{between['helper']}.jsonl"))
        written = [ts for ts, kind, event in helper if kind == "message.user" and between["text"] in event.get("text", "")]
        need(bool(written), f"the person's message to {between['helper']} reached its task")
        if between["helper"] in back and written:
            later = [ts for ts, kind, _ in helper if kind == "run.started" and when(ts) > when(written[0])]
            need(bool(later), f"{between['helper']}'s round 2 came after the person's message")
    model = bring_backs(lead, follow_up)
    # Round 2's starts are the gateway's to honour; a refused one there is its failure. Every refusal is listed below.
    in_round_two = [each for each in model["refused"] if each[0] == 2]
    need(not in_round_two, f"no round-2 start_member refused ({len(in_round_two)})")
    need(not model["not_brought_back"],
         f"every helper asked back was brought back (not: {model['not_brought_back']})")
    # The model's choice is what it asked for; whether the gateway honoured it is the check above.
    asked = {"back": ["checker", "totals"], "new": ["shares"]} if task_dir.name == "x1-region-shares" else None
    chose = bool(asked) and model["back"] == asked["back"] and model["new"] == asked["new"]
    notes.append(f"round 1 started {first}; round 2 brought back {back} and added {new}")
    notes.append(f"the lead asked back {model['back']} and new {model['new']}")
    for round_, role, code in model["refused"]:
        notes.append(f"a start_member refused in round {round_}: {role}, {code}")
    return gateway, chose, notes


def main(argv):
    tasks = Path(__file__).resolve().parent / "tasks"
    totals = {"gateway": 0, "chose": 0, "attempts": 0}
    for run in argv:
        for directory in sorted(Path(run).glob("attempts/*-team-*")):
            name = directory.name.rsplit("-team-", 1)[0]
            gateway, chose, notes = attempt(directory, tasks / name)
            ok = all(passed for passed, _ in gateway)
            totals["attempts"] += 1
            totals["gateway"] += ok
            totals["chose"] += chose
            print(f"{directory.name}: gateway {'right' if ok else 'WRONG'}; model did as asked: {chose}")
            for passed, words in gateway:
                print(f"  {'ok ' if passed else 'NO '} {words}")
            for note in notes:
                print(f"  - {note}")
    print(f"gateway right in {totals['gateway']} of {totals['attempts']}; "
          f"the model did as asked in {totals['chose']} of {totals['attempts']}")
    return 0 if totals["attempts"] and totals["gateway"] == totals["attempts"] else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
