from checklib import fail, has, numbers, read, run, table

CELLS = {
    "arcwell": {"auth": lambda c: has(c, "oauth"), "rate": lambda c: 600 in numbers(c) and has(c, "minute", "/min"),
                "pagination": lambda c: has(c, "cursor"), "pricing": lambda c: 0.002 in numbers(c)},
    "brisa": {"auth": lambda c: has(c, "api key", "x-brisa-key"),
              "rate": lambda c: 50 in numbers(c) and has(c, "second", "/s"),
              "pagination": lambda c: has(c, "offset"), "pricing": lambda c: 49 in numbers(c) and has(c, "month")},
    "corvane": {"auth": lambda c: has(c, "hmac", "signed"), "rate": lambda c: 10000 in numbers(c) and has(c, "day"),
                "pagination": lambda c: has(c, "page") and not has(c, "cursor", "offset"),
                "pricing": lambda c: 1000 in numbers(c) and 0.01 in numbers(c)},
}
COLUMNS = {"auth": ("auth",), "rate": ("rate", "limit"), "pagination": ("pagin",), "pricing": ("pric", "cost")}

def check(workspace):
    rows = table(read(workspace, "comparison.md"))
    if len(rows) < 2:
        fail("comparison.md has no markdown table")
    header = [cell.lower() for cell in rows[0]]
    where = {}
    for column, words in COLUMNS.items():
        matches = [index for index, cell in enumerate(header) if any(word in cell for word in words)]
        if not matches:
            fail(f"the table has no {column} column")
        where[column] = matches[0]
    wrong = []
    for vendor, cells in CELLS.items():
        row = next((row for row in rows[1:] if row and vendor in row[0].lower()), None)
        if row is None:
            fail(f"the table has no row for {vendor}")
        for column, good in cells.items():
            cell = row[where[column]] if where[column] < len(row) else ""
            if not good(cell):
                wrong.append(f"{vendor} {column}: {cell!r}")
    if wrong:
        fail(f"{12 - len(wrong)} of 12 cells match the documentation", "\n".join(wrong))
    return "all 12 cells match the vendors' current documentation"

run(check)
