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
    """A rounds task's team session (ADR-0034's rounds amendment): round 1, the lead starts the scout, which reports;
    the lead writes the first ask's answer (the task's `wrong` files, all round 1 asked for) and calls done. The
    person's follow-up then starts the lead's next run, which brings the scout back by its name with its copy kept
    (round 2); it reports again, and the lead writes the full answer and calls done."""
    lead = phrase(task.statement)
    helper = f"You are {HELPER}"

    def brief(objective):
        return {"objective": objective, "output": "One line to the lead.", "sources": None,
                "boundaries": "Don't edit the lead's files."}

    def report(words):
        return call(helper, "send_to_lead", {"type": "report", "text": words, "question_seq": None, "files": None})

    responses = [
        call(lead, "start_member", {"role": HELPER, "workspace": "own_copy", "profile": None,
                                    "brief": brief(f"Read the data for {task.id} and report what you find.")}),
        text(lead, "I started a helper; I'll continue when it reports."),
        call(lead, "read_reports", {"wait": 30, "limit": None}),
        *writes(lead, task.wrong),
        call(lead, "done", {"summary": f"Round 1: wrote {', '.join(sorted(task.wrong))}."}),
        text(lead, "Round 1 is done."),
        # The person's follow-up: this run opens round 2 and brings the scout back by its name.
        call(lead, "start_member", {"role": HELPER, "workspace": "own_copy", "profile": None, "files": "keep",
                                    "brief": brief(f"Check the data for {task.id} again for the follow-up.")}),
        text(lead, "I brought the helper back; I'll continue when it reports."),
        call(lead, "read_reports", {"wait": 30, "limit": None}),
        *writes(lead, files(task)),
        call(lead, "done", {"summary": f"Round 2: wrote {', '.join(sorted(files(task)))}."}),
        text(lead, "Round 2 is done."),
        report("I read the data."),
        text(helper, "I reported to my lead."),
        report("I checked the data again."),
        text(helper, "I reported to my lead again."),
    ]
    return {"model": "fixture", "select": "prompt", "responses": responses}
