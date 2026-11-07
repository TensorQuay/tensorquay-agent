import re
import subprocess
import sys
from checklib import HERE, expected, fail, read, run, visible

def check(workspace):
    visible(workspace)
    audit = read(workspace, "AUDIT.md")
    found = [path for path in expected()["defects"] if path in audit]
    if len(found) < 10:
        fail(f"AUDIT.md locates {len(found)} of the 12 defects (10 needed)")
    added = [path for path in (workspace / "tests").glob("test_*.py") if path.name != "test_helpers.py"]
    if not added:
        fail("no new test for the fixes in tests/")
    hidden = str(HERE / "hidden")
    result = subprocess.run([sys.executable, "-W", "ignore", "-m", "unittest", "discover", "-v", "-s", hidden, "-t",
                             hidden], cwd=workspace, capture_output=True, text=True, timeout=180,
                            env={"PYTHONPATH": str(workspace), "PYTHONDONTWRITEBYTECODE": "1", "PATH": "/usr/bin:/bin"})
    passed = re.findall(r"test_defect_(\d+)\S* \(.*\) \.\.\. ok", result.stdout + result.stderr)
    if len(passed) < 6:
        fail(f"{len(passed)} defects are fixed (6 needed)", result.stdout + result.stderr)
    return f"{len(found)} of 12 defects located and {len(passed)} fixed, the repository's tests passing"

run(check)
