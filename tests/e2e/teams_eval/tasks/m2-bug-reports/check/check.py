import re
from checklib import fail, held_out, run, visible

def check(workspace):
    tests = " ".join(path.read_text() for path in (workspace / "tests").rglob("*.py"))
    for name in ("percent", "median"):
        if name not in tests:
            fail(f"no test reproduces the {name} issue")
    marked = r"@unittest\.expectedFailure\s+def\s+\w+\(self\):(.*?)(?=\n\s*(?:@|def |class )|\Z)"
    expected_failures = re.findall(marked, tests, re.S)
    if not any("mean" in body for body in expected_failures):
        fail("the mean issue has no test marked expected to fail")
    visible(workspace)
    held_out(workspace)
    return "percent and median fixed with tests; mean unchanged, its test marked expected to fail"

run(check)
