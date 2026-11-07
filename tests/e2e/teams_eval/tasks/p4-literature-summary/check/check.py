from checklib import expected, fail, has, read, run, sections

def check(workspace):
    truth = expected()
    text = read(workspace, "summary.md")
    headings = [heading.lower() for heading, _ in sections(text)]
    for needed in ("agree", "gap"):
        if not any(needed in heading for heading in headings):
            fail(f"summary.md has no section on {needed}s")
    uncited = [paper["file"] for paper in truth["papers"]
               if not has(text, paper["file"], paper["file"].rsplit(".", 1)[0], paper["title"])]
    if uncited:
        fail(f"{len(uncited)} paper(s) not cited", ", ".join(uncited))
    claims = [words for paper in truth["papers"] for words in paper["claims"]]
    found = sum(1 for words in claims if has(text, *words))
    message = f"all 8 papers cited, {found} of {len(claims)} key claims"
    if found < truth["pass_at"]:
        fail(message)
    return message

run(check)
