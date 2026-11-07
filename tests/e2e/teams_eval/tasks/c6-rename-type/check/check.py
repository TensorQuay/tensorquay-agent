import re
from checklib import fail, held_out, run, visible

OLD = re.compile(r"\bRecords?\b")
KEEP = ("alias", "deprecat", "renamed", "stockitem")

def check(workspace):
    left = []
    for path in sorted((workspace / "inventory").rglob("*.py")) + sorted((workspace / "docs").rglob("*.md")):
        for number, line in enumerate(path.read_text().splitlines(), start=1):
            if OLD.search(line) and not any(word in line.lower() for word in KEEP):
                left.append(f"{path.relative_to(workspace)}:{number}: {line.strip()}")
    if left:
        fail("Record is still used outside the alias", "\n".join(left[:20]))
    visible(workspace)
    held_out(workspace)
    return "renamed to StockItem in the package and docs, the old name kept as an alias, tests pass"

run(check)
