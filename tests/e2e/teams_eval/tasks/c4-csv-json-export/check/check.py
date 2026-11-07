from checklib import fail, held_out, read, run, visible

def check(workspace):
    tests = " ".join(path.read_text().lower() for path in (workspace / "tests").rglob("*.py"))
    if "csv" not in tests or "json" not in tests:
        fail("no test covers the CSV and the JSON output")
    readme = read(workspace, "README.md").lower()
    if "--format" not in readme or "json" not in readme or "csv" not in readme:
        fail("README.md does not document --format csv and json")
    visible(workspace)
    held_out(workspace)
    return "text unchanged, CSV and JSON correct, other formats rejected, tested and documented"

run(check)
