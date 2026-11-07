from checklib import expected, fail, read, run, table

def check(workspace):
    truth = expected()
    rows = table(read(workspace, "breaking.md"))
    if len(rows) < 2:
        fail("breaking.md has no markdown table")
    texts = [(row[0] if row else "", " ".join(row).lower()) for row in rows[1:]]
    found = [token for version, token in truth["breaking"]
             if any(version in first and token in text for first, text in texts)]
    false = [token for version, token in truth["deprecated"]
             if any(version in first and token in text for first, text in texts)]
    message = f"{len(found)} of {len(truth['breaking'])} breaking changes found, {len(false)} deprecation(s) listed"
    if len(found) < truth["pass_at"] or len(false) > truth["false_at_most"]:
        missing = [token for _, token in truth["breaking"] if token not in found]
        fail(message, "missing: " + ", ".join(missing))
    return message

run(check)
