"""The Gate 7 demo's two scripted agents (WEB-E2E-012, TK-U-020): standalone fake-model scenarios for the Planner and
the Builder, in the choreography of tests/e2e/gate7_evidence.py. Each agent waits (by type) before it acts, so every seq
it names is fixed: 1 text P, 2 proposal P, 3 proposal B, 4 B accepts 2, 5 the person accepts 2, 6 decision, 7 the person
vetoes 3, 8 decision, 9 task P (two claimants), 10 P claims 9, 11 P reports, 12 the person claims 9, 13 the person
reports, 14 P done.

    python3 tests/e2e/gate7_scenarios.py --channel <id> --out <dir>   # writes planner.json and builder.json
"""
import argparse
import json
from pathlib import Path

from gate7_evidence import SCRIPT  # The words, one set for the page spec, these scenarios and the checks.

# The agents' closing answers are not channel posts, so the choreography doesn't fix them.
ANSWERS = {
    "Planner": "Approach A was accepted, approach B vetoed; the task is claimed, reported and done.",
    "Builder": "I voted for approach A; it was accepted, and my approach B was vetoed.",
}


def _call(name, arguments, prompt, completion):
    return {"events": [{"type": "tool_call", "name": name, "arguments": arguments, "split_into": 1}],
            "usage": {"prompt_tokens": prompt, "completion_tokens": completion}}


def _answer(text, prompt):
    return {"events": [{"type": "text", "text": text}], "usage": {"prompt_tokens": prompt, "completion_tokens": 30}}


def scenarios(channel_id):
    """{'Planner': scenario, 'Builder': scenario} for a channel. Posts carry no parent, and their bodies are exactly the
    choreography's LOG entries (no window or refs beyond them)."""
    c, t = channel_id, SCRIPT
    wait = lambda since, kind: {"channel": c, "since": since, "types": [kind], "timeout_seconds": 300}
    planner = [
        _call("channel_post", {"channel": c, "type": "text", "text": t["hello"]}, 400, 40),
        _call("channel_post", {"channel": c, "type": "proposal", "title": t["proposal"], "text": t["proposal_text"]},
              500, 60),
        _call("channel_wait", wait(2, "decision"), 600, 20),
        _call("channel_wait", wait(6, "decision"), 700, 20),
        _call("channel_post", {"channel": c, "type": "task", "title": t["task"], "spec": t["spec"], "claims": 2},
              800, 50),
        _call("task_claim", {"channel": c, "task_seq": 9}, 900, 20),
        _call("channel_post", {"channel": c, "type": "report", "task_seq": 9, "status": "completed",
                               "summary": t["report"]}, 1000, 40),
        _call("channel_wait", wait(11, "report"), 1100, 20),
        _call("channel_post", {"channel": c, "type": "done", "summary": t["done"]}, 1200, 40),
        _answer(ANSWERS["Planner"], 1300),
    ]
    builder = [
        _call("channel_wait", wait(0, "proposal"), 300, 20),
        _call("channel_post", {"channel": c, "type": "proposal", "title": t["counter"], "text": t["counter_text"]},
              400, 50),
        _call("channel_post", {"channel": c, "type": "vote", "proposal_seq": 2, "choice": "accept",
                               "reason": t["reason"]}, 500, 30),
        _call("channel_wait", wait(4, "decision"), 600, 20),
        _call("channel_wait", wait(6, "decision"), 700, 20),
        _answer(ANSWERS["Builder"], 800),
    ]
    return {"Planner": {"model": "fixture", "responses": planner},
            "Builder": {"model": "fixture", "responses": builder}}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--channel", required=True)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    for name, scenario in scenarios(args.channel).items():
        (args.out / (name.lower() + ".json")).write_text(json.dumps(scenario, indent=1, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
