from checklib import expected, fail, numbers, read, run, table

def check(workspace):
    truth = expected()
    rows = table(read(workspace, "regions.md"))
    if len(rows) < 2:
        fail("regions.md has no markdown table")
    wrong = []
    for region, total in truth["totals"].items():
        row = next((row for row in rows[1:] if row and region in row[0].lower()), None)
        if row is None or len(row) < 3:
            fail(f"the table has no row for {region} with a total and a share")
        values = numbers(row[1])
        if not values or abs(values[0] - total) > 0.005:
            wrong.append(f"{region}: total {row[1]!r}, the data gives {total:.2f}")
        shares = numbers(row[2])
        if not shares or abs(shares[0] - truth["shares"][region]) > 1:
            wrong.append(f"{region}: share {row[2]!r}, the data gives {truth['shares'][region]} %")
    if wrong:
        fail(f"{len(wrong)} value(s) wrong", "\n".join(wrong))
    return "every region's total is exact and its share within a point"

run(check)
