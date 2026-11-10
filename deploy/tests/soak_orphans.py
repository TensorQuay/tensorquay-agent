"""The end-of-run orphan audit (SOAK-001, PRD M5 "no orphan sandboxes or volumes"). After the drain, what exists
must be exactly what the soak's live conversations account for:
- every sandbox container of this installation is running and belongs to one of the soak's conversations, one each;
- every running sandbox has its admission row, and every row is live with its sandbox running;
- every per-sandbox network has a container, and no labelled volume is left unused.
The listings come from the stack; this module only judges them."""
import collections


def parse_rows(text):
    """`conversation|phase` lines from psql -At."""
    rows = []
    for line in text.splitlines():
        if line.strip():
            conversation, phase = line.strip().split("|", 1)
            rows.append({"conversation": conversation, "phase": phase})
    return rows


def findings(containers, rows, networks, volumes, conversations):
    problems = []
    running = collections.Counter()
    for item in containers:
        if item["conversation"] not in conversations:
            problems.append("container {} belongs to no conversation of this soak".format(item["id"]))
        elif item["state"] != "running":
            problems.append("container {} (conversation {}) is {}".format(item["id"], item["conversation"],
                                                                          item["state"]))
        else:
            running[item["conversation"]] += 1
    for conversation, count in sorted(running.items()):
        if count > 1:
            problems.append("conversation {} has {} running sandboxes".format(conversation, count))
    admitted = {row["conversation"]: row["phase"] for row in rows}
    for conversation in sorted(running):
        if conversation not in admitted:
            problems.append("conversation {} has a running sandbox but no admission row".format(conversation))
    for conversation, phase in sorted(admitted.items()):
        if phase != "live":
            problems.append("admission row for conversation {} is {}".format(conversation, phase))
        if conversation not in running:
            problems.append("conversation {} has an admission row but no running sandbox".format(conversation))
    problems += ["network {} has no container".format(item["name"]) for item in networks if item["attached"] == 0]
    problems += ["volume {} is unused".format(item["name"]) for item in volumes if not item["in_use"]]
    return problems
