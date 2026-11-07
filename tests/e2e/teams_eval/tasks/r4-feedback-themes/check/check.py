from checklib import expected, fail, norm, numbers, read, run, table

def check(workspace):
    truth = expected()
    rows = table(read(workspace, "themes.md"))
    if len(rows) < 2:
        fail("themes.md has no markdown table")
    problems = []
    for theme in truth["themes"]:
        row = next((row for row in rows[1:] if row and any(word in row[0].lower() for word in theme["words"])), None)
        if row is None or len(row) < 4:
            problems.append(f"no row for {theme['name']}")
            continue
        counted = numbers(row[1])
        if not counted or abs(counted[0] - theme["count"]) > 2:
            problems.append(f"{theme['name']}: count {row[1]!r}, the comments give {theme['count']}")
        quotes = [norm(cell).strip('"\u201c\u201d\' ') for cell in row[2:4]]
        comments = [norm(text) for text in theme["comments"]]
        if not all(quote and any(quote in comment for comment in comments) for quote in quotes):
            problems.append(f"{theme['name']}: a quote is not a comment of that theme")
    if problems:
        fail(f"{len(problems)} problem(s) in the themes", "\n".join(problems))
    return "five themes with counts within two of the comments' and real quotes"

run(check)
