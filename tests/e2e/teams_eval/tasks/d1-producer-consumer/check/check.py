import json
import subprocess
import sys
from checklib import HERE, expected, fail, has, held_out, read, run, visible

def check(workspace):
    held_out(workspace)
    visible(workspace)
    # data/ through the workspace's own writer, then its own reader's command line: the summary must be the data's.
    script = ("import json, sys; from writer.evlog_writer import write_events; "
              "events = [json.loads(l) for l in open('data/events.jsonl', encoding='utf-8') if l.strip()]; "
              "write_events(sys.argv[1], events)")
    log = workspace / ".check-events.evl"
    try:
        writer = subprocess.run([sys.executable, "-c", script, str(log)], cwd=workspace, capture_output=True,
                                text=True, timeout=60)
        if writer.returncode != 0:
            fail("writer/ can't write data/events.jsonl", writer.stderr)
        reader = subprocess.run([sys.executable, "-m", "report.evlog_report", str(log)], cwd=workspace,
                                capture_output=True, text=True, timeout=60)
    finally:
        log.unlink(missing_ok=True)
    if reader.returncode != 0:
        fail("report/'s command line can't read what writer/ wrote from data/", reader.stderr)
    want = expected()["summary"]
    if json.loads(reader.stdout) != want:
        fail("data/events.jsonl's 500 events don't come back through writer/ and report/ as they went in")
    if json.loads(read(workspace, "out/summary.json")) != want:
        fail("out/summary.json isn't the summary of data/events.jsonl")
    spec = read(workspace, "spec/FORMAT.md")
    decisions = spec.split("## Decisions", 1)[-1]
    if "to be settled" in decisions.lower() or not (has(decisions, "length", "prefix") and has(decisions, "trailer")):
        fail("spec/FORMAT.md's Decisions section doesn't record the length prefix and the trailer")
    return "the writer's files round-trip through the reader, data/ comes back exactly, and the decisions are written"

run(check)
