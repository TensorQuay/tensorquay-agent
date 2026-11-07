#!/usr/bin/env python3
"""Finite Git process witness; records argv/environment without invoking Git."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

root = Path(__file__).parent
script = json.loads((root / "script.json").read_text())
arguments = sys.argv[1:]
operation = next(value for value in ("config", "symbolic-ref", "rev-parse", "cat-file", "push") if value in arguments)
git_dir = Path(arguments[arguments.index("--git-dir") + 1])
record = {"arguments": arguments, "environment": dict(os.environ), "operation": operation,
          "git_dir": str(git_dir), "configuration": (git_dir / "config").read_text()}
with (root / "seen.jsonl").open("a") as output:
    output.write(json.dumps(record) + "\n")
mode = script.get("mode")
if mode == "stall":
    child = subprocess.Popen(["sleep", "300"])
    def terminate(_signal, _frame):
        child.terminate()
        child.wait()
        (root / "stopped").write_text("joined")
        raise SystemExit(1)
    signal.signal(signal.SIGTERM, terminate)
    (root / "started").write_text(str(os.getpid()))
    signal.pause()
elif mode == "output":
    stream = sys.stderr if script["stderr"] else sys.stdout
    stream.write("x" * script["bytes"])
    stream.flush()
elif mode == "failure":
    sys.stderr.write("test-private-diagnostic")
    raise SystemExit(1)
else:
    values = {"config": "remote.origin.url\nhttps://git.example/repo.git\0", "symbolic-ref": "refs/heads/feature/example\n",
              "rev-parse": "b" * 40 + "\n", "cat-file": "commit\n", "push": ""}
    sys.stdout.write(script.get(operation, values[operation]))
