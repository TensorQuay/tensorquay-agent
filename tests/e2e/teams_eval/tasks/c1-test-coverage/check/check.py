import json
import subprocess
import sys
import tempfile
import types
from checklib import HERE, fail, run

MODULES = ("lib/textstats.py", "lib/money.py", "lib/dates.py")

def executable(path):
    found, stack = set(), [compile(path.read_text(), str(path), "exec")]
    while stack:
        code = stack.pop()
        found.update(line for _, _, line in code.co_lines() if line)
        stack.extend(constant for constant in code.co_consts if isinstance(constant, types.CodeType))
    return found

def check(workspace):
    with tempfile.TemporaryDirectory() as scratch:
        out = f"{scratch}/coverage.json"
        result = subprocess.run([sys.executable, str(HERE / "covrun.py"), str(workspace), out, *MODULES],
                                cwd=workspace, capture_output=True, text=True, timeout=120)
        if result.returncode != 0:
            fail("the tests could not be run under the tracer", result.stderr)
        found = json.loads(open(out).read())
    if not found["passed"] or not found["ran"]:
        fail("the workspace's tests fail", result.stderr)
    shares = {}
    for name in MODULES:
        lines = executable(workspace / name)
        shares[name] = len(lines & set(found["hit"][name])) / len(lines)
    summary = ", ".join(f"{name} {share:.0%}" for name, share in shares.items())
    if min(shares.values()) < 0.8:
        fail(f"line coverage under 80 %: {summary}")
    return f"tests pass; line coverage {summary}"

run(check)
