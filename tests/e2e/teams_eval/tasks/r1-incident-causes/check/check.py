import re
from checklib import fail, read, run, sections, expected

def starts_a_word(words, heading):
    return any(re.search(r"\b" + re.escape(word), heading) for word in words)

def names(key, heading, words):
    """Whether a heading names the cause: one of its words starts a word in it, or sits inside one ("misconfigured")
    while no other cause's word starts one: "DNS misconfiguration" is DNS, not a config push (night 1, 11 Oct)."""
    heading = heading.lower()
    if starts_a_word(words[key], heading):
        return True
    return (any(word in heading for word in words[key])
            and not any(starts_a_word(other, heading) for name, other in words.items() if name != key))

def check(workspace):
    truth = expected()
    found = sections(read(workspace, "causes.md"))
    if len(found) < 5:
        fail(f"causes.md has {len(found)} '## ' cause headings, not five")
    for position, (key, (heading, body)) in enumerate(zip(truth["order"], found[:5]), start=1):
        if not names(key, heading, truth["words"]):
            fail(f"cause {position} should be {truth['names'][key]}, found {heading!r}")
        cited = set(re.findall(r"incidents/[a-z]+/INC-\d{3}\.md", body))
        real = [path for path in cited if (workspace / path).is_file() and truth["causes"].get(path) == key]
        if len(real) < 3:
            fail(f"{heading!r} cites {len(real)} real incident(s) with that cause, not three")
    return "the five causes in order, each with three real incidents of that cause"

run(check)
