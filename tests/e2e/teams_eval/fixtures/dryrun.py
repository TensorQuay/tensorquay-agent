"""The scripted model's sessions for each task (dry-run/single.json and dry-run/team.json), so the runner and both
arms run on the stack before GPU time. Both solve the task with its reference answer; neither judges anything.

Replies are chosen by prompt (tqa-testkit's scenario `select: prompt`): the lead's by a phrase of task.md, the
helper's by its first message ("You are scout, a helper ..."), each then by its own turn, so the two never race.

- **single:** the agent writes the reference answer's files and ends with a short text.
- **team:** the lead starts one helper and ends its run. The helper writes `scout-notes.md` and reports it. The
  report wakes the lead, which reads its reports, takes the helper's file (the report's seq filled in from its history
  by `seq_of`), writes the reference answer and calls `done`. The end note then closes the team.
"""

HELPER = "scout"
USAGE = {"prompt_tokens": 900, "completion_tokens": 60}


def phrase(statement):
    """A phrase of the task's statement for selecting the lead's replies: its first line, as task.md writes it."""
    return statement.strip().splitlines()[0].strip()


def call(when, name, arguments):
    return {"when": when, "events": [{"type": "tool_call", "name": name, "arguments": arguments, "split_into": 2}],
            "usage": USAGE}


def text(when, words):
    return {"when": when, "events": [{"type": "text", "text": words}], "usage": USAGE}


def writes(when, files):
    return [call(when, "write_file", {"path": path, "content": content, "create_parents": True})
            for path, content in sorted(files.items())]


def setup(when, task):
    return [call(when, "shell", {"command": command}) for command in task.dry_run_setup]


def files(task):
    return task.dry_run_files if task.dry_run_files is not None else task.solution


def single(task):
    lead = phrase(task.statement)
    return {"model": "fixture", "select": "prompt",
            "responses": setup(lead, task) + writes(lead, files(task))
            + [text(lead, f"Done: {', '.join(sorted(files(task)))}.")]}


def team(task):
    lead = phrase(task.statement)
    helper = f"You are {HELPER}"
    brief = {"objective": f"Write {HELPER}-notes.md listing the files in this workspace for {task.id}.",
             "output": f"{HELPER}-notes.md, one line per file.", "sources": None,
             "boundaries": f"Write only {HELPER}-notes.md."}
    responses = [
        call(lead, "start_member", {"role": HELPER, "workspace": "own_copy", "profile": None, "brief": brief}),
        text(lead, "I started a helper; I'll continue when it reports."),
        # The helper's report wakes the lead: its second run starts here.
        call(lead, "read_reports", {"wait": 30, "limit": None}),
        call(lead, "take_files", {"report_seq": {"seq_of": {"type": "report", "from": HELPER}}, "files": None}),
        *setup(lead, task),
        *writes(lead, files(task)),
        call(lead, "done", {"summary": f"Wrote {', '.join(sorted(files(task)))}."}),
        text(lead, f"Done: {', '.join(sorted(files(task)))}."),
        call(helper, "shell", {"command": f"find . -path ./.git -prune -o -type f -print | sort > {HELPER}-notes.md"}),
        call(helper, "send_to_lead", {"type": "report", "text": f"{HELPER}-notes.md lists the workspace's files.",
                                      "question_seq": None, "files": [f"{HELPER}-notes.md"]}),
        text(helper, "I reported to my lead."),
    ]
    return {"model": "fixture", "select": "prompt", "responses": responses}



def rounds(task):
    """A rounds task's team session (ADR-0034's rounds amendment; plan item E's bar). Round 1: the lead starts the
    task's two helpers, reads their reports, writes the first ask's answer (the task's `wrong` files) and calls done.
    Between the rounds the person writes to one helper, which answers in its own run. The follow-up starts the lead's
    next run, which brings both helpers back by their names with their copies kept and starts one new helper (round
    2); each reports, and the lead writes the full answer and calls done. Each helper's replies are chosen by "<role>,
    a helper", which its first frame and its round frame both hold."""
    lead = phrase(task.statement)
    first, new = task.rounds_roles["round1"], task.rounds_roles["new"]

    def brief(objective):
        return {"objective": objective, "output": "One line to the lead.", "sources": None,
                "boundaries": "Don't edit the lead's files."}

    def start(role, objective, files=None):
        arguments = {"role": role, "workspace": "own_copy", "profile": None, "brief": brief(objective)}
        if files:
            arguments["files"] = files
        return call(lead, "start_member", arguments)

    read = call(lead, "read_reports", {"wait": 30, "limit": None})
    responses = [
        *(start(role, f"Work on {task.id} as the {role}.") for role in first),
        text(lead, "I started the helpers; I'll read their reports."),
        read, read,
        *writes(lead, task.wrong),
        call(lead, "done", {"summary": f"Round 1: wrote {', '.join(sorted(task.wrong))}."}),
        text(lead, "Round 1 is done."),
        # The person's follow-up: this run opens round 2, brings both back by name and adds the new helper.
        *(start(role, f"Pick up {task.id} again as the {role}.", files="keep") for role in first),
        start(new, f"Join {task.id} as the {new}."),
        text(lead, "Two helpers are back and one is new; I'll read their reports."),
        read, read, read,
        *writes(lead, files(task)),
        call(lead, "done", {"summary": f"Round 2: wrote {', '.join(sorted(files(task)))}."}),
        text(lead, "Round 2 is done."),
    ]
    written = task.between["helper"] if task.between else None
    for role in first:
        helper = f"{role}, a helper"
        responses += [call(helper, "send_to_lead", {"type": "report", "text": f"The {role} reports.",
                                                    "question_seq": None, "files": None}),
                      text(helper, "I reported to my lead.")]
        if role == written:
            # Its own run between the rounds, answering the person.
            responses.append(text(helper, "Noted; I'll use that when my lead brings me back."))
        responses += [call(helper, "send_to_lead", {"type": "report", "text": f"The {role} reports again.",
                                                    "question_seq": None, "files": None}),
                      text(helper, "I reported to my lead again.")]
    responses += [call(f"{new}, a helper", "send_to_lead", {"type": "report", "text": f"The {new} reports.",
                                                             "question_seq": None, "files": None}),
                  text(f"{new}, a helper", "I reported to my lead.")]
    return {"model": "fixture", "select": "prompt", "responses": responses}
