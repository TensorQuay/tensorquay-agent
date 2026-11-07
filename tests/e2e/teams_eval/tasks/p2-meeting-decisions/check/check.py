from checklib import expected, fail, read, run, sections, table

def check(workspace):
    truth = expected()
    text = read(workspace, "decisions.md")
    rows = [" ".join(row).lower() for row in table(text)[1:]]
    if not rows:
        fail("decisions.md has no markdown table")
    found = [d["word"] for d in truth["decisions"]
             if any(d["word"] in row and d["owner"].lower() in row for row in rows)]
    stale = any("postgres" in row and "mysql" not in row for row in rows)
    open_text = " ".join(body.lower() for heading, body in sections(text) if "open" in heading.lower())
    questions = [word for word in truth["questions"] if word in open_text]
    message = f"{len(found)} of 15 decisions with their owners, {len(questions)} of 5 open questions"
    if len(found) < truth["pass_at"] or len(questions) < 4 or stale:
        detail = "the reversed Postgres decision is listed as standing" if stale else ""
        fail(message, detail)
    return message

run(check)
