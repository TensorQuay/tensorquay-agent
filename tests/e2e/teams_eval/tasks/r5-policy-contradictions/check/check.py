from checklib import expected, fail, read, run, sections

def check(workspace):
    truth = expected()
    answer = read(workspace, "contradictions.md")
    found_sections = [(heading + " " + body).lower() for heading, body in sections(answer)]
    found = [item["topic"] for item in truth["contradictions"]
             if any(any(word in text for word in item["words"]) and all(doc in text for doc in item["documents"])
                    for text in found_sections)]
    message = f"{len(found)} of {len(truth['contradictions'])} contradictions found"
    if len(found) < truth["pass_at"]:
        missing = [item["topic"] for item in truth["contradictions"] if item["topic"] not in found]
        fail(message, "missing: " + ", ".join(missing))
    return message

run(check)
