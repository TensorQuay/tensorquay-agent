import re
from checklib import expected, fail, numbers, read, run, table

def check(workspace):
    truth = expected()
    text = read(workspace, "report.md")
    rows = table(text)
    if len(rows) < 2:
        fail("report.md has no markdown table")
    wrong = []
    for region, total in truth["totals"].items():
        row = next((row for row in rows[1:] if row and region in row[0].lower()), None)
        if row is None or len(row) < 2:
            fail(f"the table has no row for {region}")
        values = numbers(row[1])
        if not values or abs(values[0] - total) > 0.005:
            wrong.append(f"{region}: {row[1]!r}, the data gives {total:.2f}")
    if wrong:
        fail(f"{len(wrong)} regional total(s) wrong", "\n".join(wrong))
    images = re.findall(r"!\[[^\]]*\]\(([^)\s]+)\)", text)
    charts = [name for name in images if (workspace / name).is_file() and (workspace / name).stat().st_size > 0]
    if not charts and "```mermaid" not in text:
        fail("report.md has no chart: no image file it links to, and no mermaid block")
    return "every region's Q3 total is exact, with a chart"

run(check)
