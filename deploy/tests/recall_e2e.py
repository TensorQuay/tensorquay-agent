"""E2E-RECALL-001: a long task looks back past its summary, on a real model on the real stack.

Each run is one task on a small window (TQA_SOAK_MODEL_CONTEXT, 12,288 by default), so it is summarised within a few
turns. The first message sets the work; two turns later the person mentions a code word in passing, a different one
each run; then more work, one section of a report per turn, until the task has been summarised at least twice. Then
the person asks for the code word. A run passes when the answer holds that word and no other code word. The record
also says whether the model searched the task's history file (`.tqa/<id>/history.md`) on the way, and how many
summaries there were.

Usage (the soak's real-model settings and the images under test):
  TQA_DOCKER_TESTS=1 TQA_SOAK_MODEL_URL=... TQA_SOAK_MODEL_NAME=... TQA_SOAK_MODEL_KEY_FILE=... \\
  TQA_GATEWAY_IMAGE=<id> TQA_WEB_IMAGE=<id> TQA_RUNTIME_IMAGE=<id> TQA_FAKE_MODEL_IMAGE=<id> \\
  python3 deploy/tests/recall_e2e.py --output DIR [--runs 10] [--parallel 4]
"""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import re
import secrets
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "deploy" / "tools"))

import compose  # noqa: E402
from soak_stack import Stack, cleanup_actions  # noqa: E402

TOPICS = ["the history of the city's parks", "how the parks are funded", "the trees most often planted",
          "the playgrounds and who uses them", "water features and their upkeep", "volunteer groups",
          "night lighting and safety", "the parks' wildlife", "events held in the parks", "plans for new parks",
          "accessibility for wheelchairs and prams", "dogs in the parks", "the cost of maintenance", "seasonal changes",
          "public toilets and kiosks", "what visitors say in surveys"]
MAX_WORK_TURNS = len(TOPICS)
TURN_SECONDS = 600
PASS_OF = (9, 10)  # the share of runs that must be right (9 of 10), for any number of runs


def events_of(stack, conversation):
    return [item.get("event") or {} for page in stack.stored(conversation) for item in page]


def kinds(events, kind):
    return [event for event in events if event.get("type") == kind]


def send(stack, conversation, text):
    """Sends a message and waits for its run to end; returns the task's events."""
    ended = len([e for e in events_of(stack, conversation) if e.get("type") in ("run.completed", "run.failed",
                                                                                 "run.cancelled")])
    stack.request("/api/v1/conversations/{}/messages".format(conversation), {"text": text})
    deadline = time.monotonic() + TURN_SECONDS
    while time.monotonic() < deadline:
        events = events_of(stack, conversation)
        done = [e for e in events if e.get("type") in ("run.completed", "run.failed", "run.cancelled")]
        if len(done) > ended:
            return events
        time.sleep(3)
    raise TimeoutError("a turn did not end within {} s".format(TURN_SECONDS))


def last_turn(events):
    """The events after the last person message."""
    last = max(index for index, event in enumerate(events) if event.get("type") == "message.user")
    return events[last + 1:]


def one_run(stack, index, output, detail="word"):
    word = "ORCHID-{}".format(100 + secrets.randbelow(900))
    if detail == "incidental":
        # A detail said in passing inside a work message, which a summary tends to drop (the harder case).
        word = "{}-K".format(10000 + secrets.randbelow(90000))
    conversation = stack.request("/api/v1/conversations", {"title": "Recall {}".format(index),
                                                           "model_route": "local"})["id"]
    record = {"run": index, "conversation": conversation, "word": word, "turns": 0}
    try:
        send(stack, conversation, "We'll write a short report on the city's parks, one section per message, as "
                                  "files under report/. First, say in one sentence how you'll lay it out.")
        send(stack, conversation, "Write section 1 ({}) to report/section-1.md with write_file: about 300 words. "
                                  "Then reply with one sentence.".format(TOPICS[0]))
        if detail == "incidental":
            send(stack, conversation, "Write section 2 ({}) to report/section-2.md with write_file: about 300 words. "
                                      "Unrelated: the print shop's order number for this report is {}. Then reply "
                                      "with one sentence.".format(TOPICS[1], word))
        else:
            send(stack, conversation, "By the way, keep this for later: the release code word is {}. Reply only "
                                      "'noted'.".format(word))
        first = 2 if detail == "word" else 3
        for turn, topic in enumerate(TOPICS[first - 1:MAX_WORK_TURNS], start=first):
            events = send(stack, conversation, "Write section {} ({}) to report/section-{}.md with write_file: about "
                                               "300 words. Then reply with one sentence.".format(turn, topic, turn))
            record["turns"] = turn
            if len(kinds(events, "compaction")) >= 2:
                break
        question = ("What was the print shop's order number I mentioned earlier? Reply with only the number."
                    if detail == "incidental" else
                    "What was the release code word I gave you earlier? Reply with only the word.")
        events = send(stack, conversation, question)
        turn_events = last_turn(events)
        answer = "".join(event.get("text", "") for event in turn_events if event.get("type") == "text.delta")
        calls = [json.dumps(event.get("arguments")) for event in turn_events if event.get("type") == "tool.call"]
        record.update(
            compactions=len(kinds(events, "compaction")),
            answer=answer.strip()[:200],
            searched_history=any("history.md" in call for call in calls),
            tool_calls=len(calls),
            right=word in answer,
            other_word=bool(re.search(r"ORCHID-\d{3}|\d{5}-K", answer.replace(word, ""))),
        )
        record["passed"] = record["compactions"] >= 2 and record["right"] and not record["other_word"]
        (output / "run-{}.json".format(index)).write_text(json.dumps(events, indent=1) + "\n")
    except Exception as error:  # a run that breaks is a failed run, reported with why and with its events so far
        record.update(passed=False, error="{}: {}".format(type(error).__name__, str(error)[:300]))
        try:
            events = events_of(stack, conversation)
            (output / "run-{}-failed.json".format(index)).write_text(json.dumps(events, indent=1) + "\n")
            last = events[-1] if events else {}
            record.update(events=len(events), last_event=last.get("type"),
                          runs_ended=len([e for e in events if e.get("type") in ("run.completed", "run.failed")]),
                          last_text=next((e.get("text", "")[-160:] for e in reversed(events)
                                          if e.get("type") in ("text.delta", "reasoning.delta")), None))
        except Exception as reading:  # the stack itself may be the cause
            record.update(events_unreadable="{}: {}".format(type(reading).__name__, str(reading)[:200]))
    print("== {} run {}: {}".format(time.strftime("%H:%M:%S"), index, json.dumps(
        {key: record.get(key) for key in ("passed", "compactions", "searched_history", "right", "answer", "error")})),
        flush=True)
    return record


def main(argv=None):
    parser = argparse.ArgumentParser(description="E2E-RECALL-001 on a real model")
    parser.add_argument("--output", required=True)
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--parallel", type=int, default=4)
    parser.add_argument("--detail", choices=("word", "incidental"), default="word",
                        help="word: a code word to keep; incidental: an order number said in passing (harder)")
    options = parser.parse_args(argv)
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("E2E-RECALL-001 needs Docker: set TQA_DOCKER_TESTS=1")
    compose.teardown_on_signals()
    output = Path(options.output)
    output.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.setdefault("TQA_SOAK_MODEL_CONTEXT", "12288")
    temporary = tempfile.TemporaryDirectory(prefix="tqa-recall-")
    directory = Path(temporary.name) / "installation"
    directory.mkdir(mode=0o700)
    stack, failure, records = Stack(directory, env, output, model="real"), None, []
    try:
        stack.prepare(rehearsal=True)
        with concurrent.futures.ThreadPoolExecutor(max_workers=options.parallel) as pool:
            records = list(pool.map(lambda index: one_run(stack, index, output, options.detail),
                                    range(1, options.runs + 1)))
    except BaseException as error:
        failure = error
    finally:
        failure = cleanup_actions([stack.save_logs], failure)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)
    passed = sum(1 for record in records if record.get("passed"))
    pass_at = -(-len(records) * PASS_OF[0] // PASS_OF[1])  # rounded up, in integers
    result = {"id": "E2E-RECALL-001", "detail": options.detail, "runs": len(records), "passed": passed,
              "pass_at": pass_at,
              "searched_history": sum(1 for record in records if record.get("searched_history")),
              "context_window": int(env["TQA_SOAK_MODEL_CONTEXT"]), "model": env.get("TQA_SOAK_MODEL_NAME"),
              "status": "passed" if failure is None and records and passed >= pass_at else "failed",
              "failure": None if failure is None else str(failure)[:500], "records": records}
    (output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print("E2E-RECALL-001 {}: {} of {} right ({} searched the history)".format(
        result["status"], passed, len(records), result["searched_history"]), flush=True)
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
