import json
import subprocess
import sys
from checklib import HERE, expected, fail, held_out, read, run

def check(workspace):
    held_out(workspace)
    want = expected()["figures"]
    if json.loads(read(workspace, "out/weekly.json")) != want:
        fail("out/weekly.json's figures aren't the week's")
    rerun = subprocess.run([sys.executable, "-m", "pipeline.run", "logs", ".check-out", "--week", "2026-09-28"],
                           cwd=workspace, capture_output=True, text=True, timeout=120)
    try:
        if rerun.returncode != 0:
            fail("python3 -m pipeline.run fails", rerun.stderr)
        if json.loads((workspace / ".check-out" / "weekly.json").read_text()) != want:
            fail("the pipeline's own run doesn't give out/weekly.json's figures")
    finally:
        subprocess.run(["rm", "-rf", str(workspace / ".check-out")], check=False)
    return "the four stages agree: the week's 24 figures are exact, and other logs come through too"

run(check)
