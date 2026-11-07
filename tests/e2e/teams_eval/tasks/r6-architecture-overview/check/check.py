from checklib import expected, fail, read, run, sections

def check(workspace):
    truth = expected()
    found = {heading.lower(): body.lower() for heading, body in sections(read(workspace, "ARCHITECTURE.md"))}
    def section(*words):
        for heading, body in found.items():
            if any(word in heading for word in words):
                return body
        fail(f"ARCHITECTURE.md has no section on {words[0]}")
    components = section("component")
    named = [name for name in truth["components"] if name in components]
    if len(named) < truth["pass_at"]:
        fail(f"{len(named)} of {len(truth['components'])} components named")
    flow = section("data flow", "flow")
    if not all(word in flow for word in ("api", "queue", "worker")):
        fail("the data flow section does not follow a request through api, queue and worker")
    feature = section("feature", "extend")
    if "shopflow/" not in feature:
        fail("the feature section names no file to change")
    return f"{len(named)} of {len(truth['components'])} components, the flow and where to add a feature"

run(check)
