from checklib import expected, fail, read, run, table

def check(workspace):
    truth = expected()
    rows = table(read(workspace, "risks.md"))
    if len(rows) < 2:
        fail("risks.md has no markdown table")
    body = [[cell.lower() for cell in row] for row in rows[1:]]
    found, owned = 0, 0
    for risk in truth["risks"]:
        matching = [row for row in body if row and any(word in row[0] for word in risk["words"])]
        if matching:
            found += 1
            if any(risk["owner"].lower() in " ".join(row[1:]) for row in matching):
                owned += 1
    empty = [row[0] for row in body if len(row) < 4 or not row[1] or not row[2]]
    message = f"{found} of 10 risks found, {owned} with the right owner"
    if found < 9 or owned < 8 or empty:
        fail(message, ("no likelihood or impact: " + ", ".join(empty)) if empty else "")
    return message

run(check)
