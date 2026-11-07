import os
import re
import subprocess
import sys
from checklib import expected, fail, run

def check(workspace):
    tool = (workspace / "tool" / "report.py").read_text() if (workspace / "tool" / "report.py").is_file() else ""
    imports = set(re.findall(r"parsers(?:\.|\s+import\s+)(\w+_parser)", tool))
    if imports != {"csv_parser"}:
        fail(f"tool/report.py uses {sorted(imports) or 'no parser'}, not only the fastest, csv_parser")
    environment = dict(os.environ, PYTHONPATH=str(workspace), PYTHONDONTWRITEBYTECODE="1")
    result = subprocess.run([sys.executable, "-m", "tool.report"], cwd=workspace, capture_output=True, text=True,
                            timeout=60, env=environment)
    if result.returncode != 0 or result.stdout != expected()["output"]:
        fail("python3 -m tool.report no longer prints the same totals", result.stdout[-500:] + result.stderr[-500:])
    if not (workspace / "bench.py").is_file():
        fail("there is no bench.py")
    bench = subprocess.run([sys.executable, "bench.py"], cwd=workspace, capture_output=True, text=True, timeout=120,
                           env=environment)
    if bench.returncode != 0 or not all(name in bench.stdout for name in ("csv", "split", "char")):
        fail("bench.py does not run or does not report all three parsers", bench.stdout[-500:] + bench.stderr[-500:])
    return "the tool uses csv_parser, prints the same totals, and bench.py times all three"

run(check)
