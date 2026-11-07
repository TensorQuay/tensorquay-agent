"""Regenerates the compaction demo's seed/, followups/, check/expected.txt and dry-run/ deterministically:
python3 generate.py.

The demo is a fifty-message conversation (DEV-PLAN Phase 5 demo, Gate 5). The first message states the task and a
codeword; forty-eight follow-ups each ask for one ledger part, which grows the context well past a 32k window; the
last message asks for a report that must end with the codeword, which only the first message carries. The run
therefore needs compaction, and it passes only if the pinned task statement survived it (CTX-U-013).
"""
import json
import pathlib
import random
import shutil

HERE = pathlib.Path(__file__).resolve().parent
PARTS = 49
CODEWORD = "KESTREL-4471"
LINES_PER_PART = 60


def part(number, rng):
    lines, total = [], 0
    for entry in range(LINES_PER_PART):
        amount = rng.randrange(10, 9999)
        total += amount
        account = f"A-{rng.randrange(1000, 9999)}"
        lines.append(f"entry {number:02d}-{entry:03d} account {account} amount {amount:5d} memo {rng.choice(['rent', 'fees', 'refund', 'payroll', 'supplies', 'travel'])}")
    lines.append(f"TOTAL {total}")
    return "\n".join(lines) + "\n", total


def main():
    rng = random.Random(2026092650)
    for name in ("seed", "followups"):
        shutil.rmtree(HERE / name, ignore_errors=True)
    (HERE / "seed" / "ledger").mkdir(parents=True)
    (HERE / "followups").mkdir()
    totals = {}
    for number in range(1, PARTS + 1):
        text, totals[number] = part(number, rng)
        (HERE / "seed" / "ledger" / f"part-{number:02d}.txt").write_text(text)
    (HERE / "seed" / "README.md").write_text(f"# ledger\n\n{PARTS} ledger parts under `ledger/`; each ends with its TOTAL line.\n")
    (HERE / "task.md").write_text(
        "We will audit this ledger one part at a time over the next messages. Remember this for the whole "
        f"conversation: the final report must end with the line `Audit reference: {CODEWORD}`.\n\n"
        "For now, read `ledger/part-01.txt` and tell me its TOTAL line.\n")
    for number in range(2, PARTS + 1):
        (HERE / "followups" / f"{number:02d}.md").write_text(f"Read `ledger/part-{number:02d}.txt` and tell me its TOTAL line.\n")
    (HERE / "followups" / f"{PARTS + 1:02d}.md").write_text(
        f"Now write `REPORT.md`: one line `part-NN: TOTAL` for every part from 01 to {PARTS}, in order, and end it "
        "with the audit reference line I gave you at the start of this conversation.\n")
    report = "".join(f"part-{number:02d}: {totals[number]}\n" for number in range(1, PARTS + 1))
    (HERE / "check" / "expected.txt").write_text(report + f"Audit reference: {CODEWORD}\n")
    summary = {"goal": "Audit the ledger part by part, then write REPORT.md",
               "done": ["read the ledger parts so far"], "pending": ["read the remaining parts", "write REPORT.md"],
               "decisions": [], "key_paths": ["ledger/"], "open_errors": []}
    for name, closing in (("solve", f"Audit reference: {CODEWORD}\n"), ("wrong", "Audit reference: pending\n")):
        responses = []
        for number in range(1, PARTS + 1):
            responses.append({"events": [{"type": "tool_call", "name": "read_file",
                                          "arguments": {"path": f"ledger/part-{number:02d}.txt"}, "split_into": 1}],
                              "usage": {"prompt_tokens": 800 + 400 * number, "completion_tokens": 20}})
            responses.append({"events": [{"type": "text", "text": f"part-{number:02d}: TOTAL {totals[number]}"}],
                              "usage": {"prompt_tokens": 1000 + 400 * number, "completion_tokens": 15}})
        responses.append({"events": [{"type": "tool_call", "name": "write_file",
                                      "arguments": {"path": "REPORT.md", "content": report + closing, "create_parents": False},
                                      "split_into": 2}], "usage": {"prompt_tokens": 9000, "completion_tokens": 400}})
        responses.append({"events": [{"type": "text", "text": "REPORT.md is written."}],
                          "usage": {"prompt_tokens": 9500, "completion_tokens": 10}})
        scenario = {"model": "fixture", "select": "arrival",
                    "summary": {"events": [{"type": "text", "text": json.dumps(summary)}],
                                "usage": {"prompt_tokens": 6000, "completion_tokens": 120}},
                    "responses": responses}
        (HERE / "dry-run" / f"{name}.json").write_text(json.dumps(scenario, indent=1) + "\n")


if __name__ == "__main__":
    main()
