import re
from checklib import fail, held_out, run, visible

def check(workspace):
    left = [str(path.relative_to(workspace)) for path in sorted((workspace / "app").rglob("*.py"))
            if re.search(r"\bv1\b", path.read_text())]
    if left:
        fail("app/ still uses client.v1", "\n".join(left))
    visible(workspace)
    held_out(workspace)
    return "every caller uses client.v2 and behaves as before"

run(check)
