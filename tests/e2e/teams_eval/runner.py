"""The agent-teams evaluation's runs (docs/evals/agent-teams.md section 2): each task tagged `teams-eval` three times
as a single agent and three times with agent teams on, at the same budget, through our own public API, one attempt at
a time.

Each attempt:
1. creates a workspace and seeds it as tqa-e2e seeds an attempt's (the seed, one commit with a fixed author and date,
   handed to the runtime's user), from a container, because the storage belongs to that user;
2. creates the task on the route and template, and sends `task.md` verbatim; the team side gets TEAM_BRIEF after it,
   one paragraph, the same for every task;
3. follows the lead's event stream:
   - the single agent's attempt ends with its run; the run is cancelled past the task's `max_tokens` or `max_steps`;
   - the team's attempt ends when its run has ended and its agent team, if the lead started one, has ended too (a
     lead woken by a report runs again); past the same token budget, counting the helpers' tokens, or the steps, the
     run is cancelled and the team stopped;
   - at the task's wall clock plus the margin, the run is cancelled and the team stopped;
4. replays the event log, reads the team's record and messages, and downloads the workspace;
5. checks the copy with `tqa-e2e --check-only` when the run completed; only then is the task deleted.

An attempt is rerun once when the model server or the connection failed (`provider_error`, or the API not answering),
never for the agent's own failure; the rerun is listed beside the run it repeats. Sides switch with an operator's
command that restarts the gateway with `[agent_teams]` off or on; before each team attempt the agent teams' budget
and deadline are set to the task's.

A stopped run resumes with `--resume` on the same `--out` and the same settings (refused otherwise): the attempts
`attempts.jsonl` lists as done are skipped, a rerun that was owed but never ran runs, and an attempt cut off without
its line runs again, its partial evidence kept aside as `<attempt>.interrupted-<n>` and listed in `interrupted.jsonl`
(its workspace and task, if any, are left for tqa-e2e's sweep: the line that named them was never written). A
half-written last line is kept in `attempts.partial`. The first attempt after a resume switches the side, whatever
it was.

`--stop-at HH:MM` (the night's end, local time) starts no attempt whose wall budget plus 5 minutes would end after
it; the run stops cleanly there, and `--resume` goes on from the first attempt not run. The stop isn't one of the
settings a resume compares, so the next night can set its own.

`--side team` (or `single`) runs that side only: night 3 (13 Oct 2026) runs the team side of fri-d and fri-s with the
brief, since night 1's team side started no helper, and the write-up compares it with night 1's single attempts, task
by task. The sides run are a setting, so a resume keeps them.
"""

import argparse
import json
import math
import re
import socket
import subprocess
import sys
import time
import tomllib
from pathlib import Path
from typing import Callable, NamedTuple, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
from api import Api, ApiError  # noqa: E402

TAG = "teams-eval"
KIND_TAG = "teams-"
KINDS = ("research", "report", "code", "mixed", "dependent", "long")
SIDES = ("single", "team")
REPEAT = 3
MARGIN_SECS = 60
# The cutoff's room after an attempt's wall budget (`--stop-at`): the night's stop rule, 5 minutes.
STOP_MARGIN_SECS = 300
# What a resume doesn't compare: the token's file, the resume itself, and the night's stop time.
SETTINGS_IGNORED = ("token_file", "resume", "stop_at")
# What the team side's lead reads after the task (from night 2, 12 Oct 2026): on night 1 the lead was offered
# start_member on every team attempt and never used it, so the team arm measured one agent. In a person's words, what
# leading a team means; never which part to hand out, so it steers no task. run.json keeps it, and a resume compares it.
TEAM_BRIEF = ("You can lead a team on this: start helpers, each taking one part of the work from your brief and "
              "reporting back to you. Use them where splitting the work helps.")
# A quiet stream is reopened after this long, so the deadline and the team are checked even without frames.
STREAM_READ_SECS = 30
TEAM_POLL_SECS = 15
RECONNECTS = 20
SEED_DATE = "2026-01-01T00:00:00Z"
WORKSPACE_OWNER = "1000:1000"
MAX_FILE_BYTES = 256 * 1024
TERMINAL = ("run.completed", "run.failed", "run.cancelled")
# The run's failure codes that are the model's (classify.rs's model_code); provider_error is the model server's.
MODEL_CODES = ("budget", "no_progress", "repeated_failure", "fallback_parse", "decision_stop")


class Task(NamedTuple):
    id: str
    kind: str
    text: str
    max_steps: int
    wall_clock_secs: int
    max_tokens: int
    template: Optional[str]
    key_facts: tuple
    directory: Path


class Settings(NamedTuple):
    api: Api
    route: str
    seed: Callable  # (task, workspace_id) -> None
    check: Callable  # (task, workspace_dir, record_dir) -> {"outcome": ..., "note": ...}
    switch: Callable  # (side) -> None
    margin_secs: float = MARGIN_SECS
    clock: Callable = time.monotonic
    sleep: Callable = time.sleep
    wall_time: Callable = lambda: time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())  # noqa: E731
    # The epoch time the cutoff (`--stop-at`) reads.
    now: Callable = time.time
    # Before each attempt and rerun: (task, side, number). The stack dry run serves the scripted model's session here.
    prepare: Callable = lambda task, side, number: None  # noqa: E731


def select_tasks(root, only=()):
    """The tasks tagged `teams-eval`, in id order, each with its kind (a `teams-<kind>` tag) and its key facts
    (`key_facts.json`, a list of strings); `only` narrows them to those ids."""
    tasks = []
    for directory in sorted(Path(root).iterdir()):
        manifest_path = directory / "task.toml"
        if not manifest_path.is_file():
            continue
        manifest = tomllib.loads(manifest_path.read_text())
        tags = manifest.get("tags", [])
        if TAG not in tags:
            continue
        kinds = [tag[len(KIND_TAG):] for tag in tags if tag.startswith(KIND_TAG) and tag[len(KIND_TAG):] in KINDS]
        if len(kinds) != 1:
            raise SystemExit(f"{manifest['id']}: needs exactly one of the tags "
                             + ", ".join(KIND_TAG + kind for kind in KINDS))
        facts = json.loads((directory / "key_facts.json").read_text())
        if not facts or not all(isinstance(fact, str) and fact.strip() for fact in facts):
            raise SystemExit(f"{manifest['id']}: key_facts.json must list the key facts as strings")
        budgets = manifest["budgets"]
        tasks.append(Task(manifest["id"], kinds[0], (directory / "task.md").read_text(), budgets["max_steps"],
                          budgets["wall_clock_secs"], budgets["max_tokens"], manifest.get("template"),
                          tuple(facts), directory))
    if only:
        missing = sorted(set(only) - {task.id for task in tasks})
        if missing:
            raise SystemExit(f"--only names tasks not tagged {TAG}: {', '.join(missing)}")
        tasks = [task for task in tasks if task.id in only]
    return tasks


def order(tasks, repeat=REPEAT, sides=SIDES):
    """The attempts in the design's order: each task's single attempts, then its team attempts, tasks in id order;
    only the `sides` given, in that same order."""
    return [(task, side, number) for task in tasks for side in SIDES if side in sides
            for number in range(1, repeat + 1)]


def seed_args(image, seed, workspace):
    """docker arguments that seed a new workspace as tqa-e2e's docker::seed_args does: the seed copied in, the root
    kept private, one commit with a fixed author and date, everything handed to the runtime's user."""
    script = ("set -eu; cp -a /seed/. /workspace/; chmod 0700 /workspace; cd /workspace; "
              "git -c safe.directory=/workspace init -q -b main; "
              "git -c safe.directory=/workspace add -A; "
              "git -c safe.directory=/workspace -c user.name=seed -c user.email=seed@example.invalid "
              "commit -q -m seed; chown -R 1000:1000 /workspace")
    return ["run", "--rm", "--network", "none", "--user", "0:0", "--label", "tqa.e2e.container=1",
            "--mount", f"type=bind,source={seed},target=/seed,readonly",
            "--mount", f"type=bind,source={workspace},target=/workspace",
            "--env", f"GIT_AUTHOR_DATE={SEED_DATE}", "--env", f"GIT_COMMITTER_DATE={SEED_DATE}",
            "--entrypoint", "sh", image, "-c", script]


def sse_frames(response):
    """The frames of an event stream, one per `data:` event; None for a keep-alive, so the caller can look at its
    clock between frames."""
    data = []
    for raw in response:
        line = raw.decode("utf-8").rstrip("\r\n")
        if line.startswith("data:"):
            value = line[5:]
            data.append(value[1:] if value.startswith(" ") else value)
        elif line == "":
            if data:
                yield json.loads("\n".join(data))
                data = []
        elif line.startswith(":"):
            yield None


class Followed:
    """What following one attempt observed."""

    def __init__(self):
        self.conversation = None
        self.problem = None  # a driver failure: the step and the error
        self.transport = False  # the API stopped answering (a rerun reason)
        self.over_budget = False
        self.deadline_passed = False
        self.lead_tokens = 0
        self.helper_tokens = 0
        self.steps = 0
        self.runs = 0
        self.team = None
        self.wall_secs = 0.0

    def fail(self, step, error):
        if self.problem is None:
            self.problem = f"{step}: {error}"
        if isinstance(error, ApiError) and error.status is None:
            self.transport = True


def brief(task, side):
    """What an attempt sends: the task's text, and on the team side TEAM_BRIEF as its own paragraph after it."""
    return task.text if side == "single" else f"{task.text.rstrip()}\n\n{TEAM_BRIEF}"


def follow(task, side, settings, workspace):
    """Starts one attempt's task and follows it to its end (module docstring, step 3)."""
    api, clock = settings.api, settings.clock
    followed = Followed()
    started = clock()
    deadline = started + task.wall_clock_secs + settings.margin_secs
    try:
        followed.conversation = api.create_conversation(f"{task.id} {side}", settings.route, workspace,
                                                        task.template)
        api.send_message(followed.conversation, brief(task, side))
    except ApiError as error:
        followed.fail(error.step, error)
        followed.wall_secs = clock() - started
        return followed
    state = {"last": 0, "run": None, "active": False, "next_poll": started}

    def stop(reason_step):
        """Cancels the active run and, on the team's side, stops the team. A 409 means the run or the team ended
        first (a race the stop lost): already done, not the harness's."""
        if state["active"] and state["run"]:
            try:
                api.cancel(followed.conversation, state["run"])
            except ApiError as error:
                if error.status != 409:
                    followed.fail(reason_step, error)
        if side == "team":
            try:
                if api.team(followed.conversation) is not None:
                    api.stop_team(followed.conversation)
            except ApiError as error:
                if error.status != 409:
                    followed.fail(reason_step, error)

    def team_over():
        """Polls the team; whether the attempt has ended (no run active, and no team or an ended one)."""
        state["next_poll"] = clock() + TEAM_POLL_SECS
        team = api.team(followed.conversation)
        if team is not None:
            followed.helper_tokens = sum(helper.get("tokens", 0) for helper in team.get("helpers", []))
        return not state["active"] and (team is None or team["state"] == "ended")

    def over_budget():
        tokens = followed.lead_tokens + (followed.helper_tokens if side == "team" else 0)
        return tokens > task.max_tokens or followed.steps > task.max_steps

    def observe(frame):
        """Acts on one frame; returns whether the attempt ended."""
        if frame.get("seq") is not None:
            state["last"] = frame["seq"]
        if frame.get("type") == "error" and state["run"] is None:
            followed.fail("deliver the task", f"{frame.get('code')}: {frame.get('message')}")
            return True
        if frame.get("type") != "event":
            return False
        event = frame["event"]
        kind = event.get("type")
        if kind == "run.started":
            state["run"], state["active"] = event["run_id"], True
            followed.runs += 1
        elif kind == "step.completed":
            followed.steps += 1
        elif kind == "usage":
            followed.lead_tokens += event.get("prompt_tokens", 0) + event.get("completion_tokens", 0)
        elif kind in TERMINAL:
            state["active"] = False
            return side == "single" or followed.over_budget or team_over()
        if over_budget() and not followed.over_budget and state["active"]:
            followed.over_budget = True
            stop("stop over budget")
        return False

    reconnects = 0
    ended = False
    while not ended:
        now = clock()
        if now >= deadline:
            followed.deadline_passed = True
            stop("stop at the deadline")
            break
        try:
            with api.stream(followed.conversation, state["last"], min(STREAM_READ_SECS, deadline - now)) as response:
                for frame in sse_frames(response):
                    if frame is not None and observe(frame):
                        ended = True
                        break
                    if clock() >= deadline:
                        break
                    if side == "team" and clock() >= state["next_poll"]:
                        if team_over() and state["run"] is not None:
                            ended = True
                            break
                        if over_budget() and not followed.over_budget:
                            followed.over_budget = True
                            stop("stop over budget")
        except ApiError as error:
            if error.status is not None:
                followed.fail(error.step, error)
                break
            reconnects += 1
            if reconnects > RECONNECTS:
                followed.fail(error.step, error)
                break
            settings.sleep(1)
        except (TimeoutError, socket.timeout, OSError):
            # A quiet stream past its read bound: reopened from the last frame.
            pass
        except ValueError as error:
            followed.fail("read the event stream", f"undecodable frame: {error}")
            break
        if not ended and side == "team" and state["run"] is not None and not state["active"]:
            try:
                ended = team_over()
                if not ended and over_budget() and not followed.over_budget:
                    followed.over_budget = True
                    stop("stop over budget")
            except ApiError as error:
                followed.fail(error.step, error)
                break
    followed.wall_secs = clock() - started
    return followed


def walk(api, conversation, directory=""):
    """Every file of the task's workspace, as (path, bytes), the way tqa-e2e's download walks it; `.git` is left out,
    as the check and the judges never need it."""
    out = []
    pending = [directory]
    while pending:
        current = pending.pop()
        for entry in api.files(conversation, current):
            name = entry["name"]
            if "/" in name or name in ("", ".", ".."):
                raise ApiError("list the files", None, f"unsafe name {name!r}")
            path = f"{current}/{name}" if current else name
            if entry["kind"] == "dir":
                if path != ".git":
                    pending.append(path)
            elif entry["kind"] == "file":
                out.append((path, api.download(conversation, path)))
    return sorted(out)


def changed_files(seed, files):
    """{path: text} of the files the attempt added or changed against the seed, for the judges: text files up to
    256 KiB; a binary or larger file is named with a placeholder."""
    out = {}
    for path, data in files:
        original = Path(seed) / path
        if original.is_file() and original.read_bytes() == data:
            continue
        if len(data) > MAX_FILE_BYTES:
            out[path] = f"[a file of {len(data)} bytes, too large to show]\n"
            continue
        try:
            out[path] = data.decode("utf-8")
        except UnicodeDecodeError:
            out[path] = f"[a binary file of {len(data)} bytes]\n"
    return out


def answer(frames):
    """The final answer: the visible text of the last run that wrote any."""
    texts = {}
    last = None
    for frame in frames:
        event = frame.get("event") or {}
        if frame.get("type") == "event" and event.get("type") == "text.delta" and event.get("text"):
            run = frame.get("run_id")
            texts.setdefault(run, []).append(event["text"])
            last = run
    return "".join(texts.get(last, []))


def terminal(frames):
    """The last run's ending: (event type, code or None), or (None, None) when no run ended."""
    for frame in reversed(frames):
        event = frame.get("event") or {}
        if frame.get("type") == "event" and event.get("type") in TERMINAL:
            return event["type"], event.get("code")
    return None, None


def refused_arguments(frames):
    """Whether the run's last failed tool results were all our tools refusing the model's arguments (classify.rs)."""
    failures = [frame["event"].get("content", "") for frame in reversed(frames)
                if frame.get("type") == "event" and frame["event"].get("type") == "tool.result"
                and frame["event"].get("status") == "error"][:3]
    return bool(failures) and all(content.startswith("invalid_args:") for content in failures)


def tool_calls(frames, name):
    """(timestamp, arguments) of the lead's calls to one tool."""
    return [(frame.get("ts", ""), frame["event"].get("arguments") or {}) for frame in frames
            if frame.get("type") == "event" and frame["event"].get("type") == "tool.call"
            and frame["event"].get("name") == name]


# The opening tag of a block crates/tqa-tools/src/untrusted.rs writes: source, the attributes in order, then the id its
# closing tag repeats. A body can contain text that looks like an opening tag but never the closing tag with its
# block's id, so only blocks found at the top level, each skipped to its own closing tag, are real.
BLOCK_OPEN = re.compile(r'<(untrusted-content|signed-in-person) source="([a-z_]+)"((?: [a-z_]+="[^"]*")*) '
                        r'id="([^"]+)">\n')
ATTRIBUTE = re.compile(r'([a-z_]+)="([^"]*)"')


def blocks(content):
    """[(source, {attribute: value})] of the top-level marked blocks in a tool result, in order; text inside a block's
    body (untrusted, a helper's words) is never read as a block."""
    found, position = [], 0
    while (opening := BLOCK_OPEN.search(content, position)) is not None:
        tag, source, attributes, block_id = opening.groups()
        closing = content.find(f'\n</{tag} id="{block_id}">', opening.end())
        if closing < 0:
            break  # an unterminated block: nothing after its opening is trusted
        found.append((source, dict(ATTRIBUTE.findall(attributes))))
        position = closing + len(f'\n</{tag} id="{block_id}">')
    return found


def reports_read(frames):
    """The highest report seq the lead's read_reports results name: its read cursor, by the gateway's own seqs (the
    top-level team blocks the lead's model reads, source "team" and type "report"), never by comparing two writers'
    clocks, and never from a report's own text."""
    calls = {frame["event"].get("call_id") for frame in frames
             if frame.get("type") == "event" and frame["event"].get("type") == "tool.call"
             and frame["event"].get("name") == "read_reports"}
    seqs = [int(attributes["seq"]) for frame in frames
            if frame.get("type") == "event" and frame["event"].get("type") == "tool.result"
            and frame["event"].get("call_id") in calls
            for source, attributes in blocks(frame["event"].get("content", ""))
            if source == "team" and attributes.get("type") == "report" and attributes.get("seq", "").isdigit()]
    return max(seqs, default=0)


def team_failures(team, messages, frames):
    """The team-only failure classes (stats.TEAM_ONLY) the record shows:
    - helper_idle_or_stuck: a member_idle or member_stuck notice;
    - helper_never_reported: a helper with no report;
    - report_not_taken: a report with files the lead never called take_files on;
    - lead_finished_unread: the team ended by done with a report past the lead's read cursor (the highest report seq
      its read_reports results name);
    - end_note_missing: an ended team without its end note."""
    if team is None:
        return []
    found = set()
    reports = [message for message in messages if message["type"] == "report"]
    if any(message["type"] == "notice" and message.get("kind") in ("member_idle", "member_stuck")
           for message in messages):
        found.add("helper_idle_or_stuck")
    reporters = {message["from"].get("name") for message in reports}
    if any(helper["name"] not in reporters for helper in team.get("helpers", [])):
        found.add("helper_never_reported")
    taken = {arguments.get("report_seq") for _, arguments in tool_calls(frames, "take_files")}
    if any(message.get("files") and message["seq"] not in taken for message in reports):
        found.add("report_not_taken")
    if team.get("stop_reason") == "done":
        cursor = reports_read(frames)
        if any(message["seq"] > cursor for message in reports):
            found.add("lead_finished_unread")
    if team.get("state") == "ended" and not any(message["type"] == "ended" for message in messages):
        found.add("end_note_missing")
    return sorted(found)


def failures(followed, frames, team, messages, side):
    """The attempt's failure classes: on both sides model_error, tool_args, budget and deadline; on the team's also
    the team-only ones."""
    found = set()
    ending, code = terminal(frames)
    if ending == "run.failed":
        if code == "repeated_failure" and refused_arguments(frames):
            found.add("tool_args")
        elif code == "budget":
            found.add("budget")
        elif code != "provider_error":
            found.add("model_error")
    if followed.over_budget or (team or {}).get("stop_reason") == "budget":
        found.add("budget")
    if followed.deadline_passed or (team or {}).get("stop_reason") == "deadline":
        found.add("deadline")
    if side == "team":
        found.update(team_failures(team, messages, frames))
    return sorted(found)


def provider_failed(frames):
    """Whether any run in an event log failed with provider_error."""
    return any(frame.get("type") == "event" and frame["event"].get("type") == "run.failed"
               and frame["event"].get("code") == "provider_error" for frame in frames)


def rerun_reason(followed, frames, helper_frames=()):
    """Why the attempt is rerun, or None: the model server failed (provider_error), for the lead or for any helper,
    since a team makes more model calls and would otherwise meet more of the server's failures; or the API stopped
    answering."""
    if terminal(frames) == ("run.failed", "provider_error"):
        return "provider_error"
    if any(provider_failed(log) for log in helper_frames):
        return "provider_error (helper)"
    if followed.transport:
        return "connection"
    return None


def run_attempt(task, side, number, out, settings):
    """One attempt (module docstring); answers its line for attempts.jsonl, and leaves its evidence under
    `out`/attempts/<task>-<side>-<number>/."""
    api = settings.api
    directory = Path(out) / "attempts" / f"{task.id}-{side}-{number}"
    directory.mkdir(parents=True)
    line = {"task": task.id, "kind": task.kind, "side": side, "attempt": number}
    workspace = None
    try:
        workspace = api.create_workspace(f"teams-eval-{task.id}-{side}-{number}")
        line["workspace"] = workspace
        settings.seed(task, workspace)
        if side == "team":
            api.set_team_limits(task.max_tokens, math.ceil((task.wall_clock_secs + settings.margin_secs) / 60))
    except (ApiError, subprocess.SubprocessError, OSError) as error:
        # The attempt never started: the harness's, listed and never counted; a dropped connection is rerun.
        if workspace:
            try:
                api.delete_workspace(workspace)
            except ApiError:
                pass
        transport = isinstance(error, ApiError) and error.status is None
        return {**line, "problem": f"set up the attempt: {error}", "rerun": "connection" if transport else None,
                "passed": None, "failures": [], "tokens": 0, "wall_secs": 0.0, "helpers": 0, "helper_names": []}
    followed = follow(task, side, settings, workspace)
    frames, team, messages, files, helper_frames = [], None, [], [], {}
    if followed.conversation:
        try:
            frames = api.events(followed.conversation)
            team = api.team(followed.conversation)
            messages = api.team_messages(followed.conversation) if team is not None else []
            files = walk(api, followed.conversation)
        except ApiError as error:
            followed.fail(error.step, error)
        for helper in (team or {}).get("helpers", []):
            # Each helper is a task of the person's: its log says whether the model server failed it.
            try:
                helper_frames[helper["name"]] = api.events(helper["conversation"])
            except (ApiError, KeyError):
                helper_frames[helper.get("name", "?")] = []
    with open(directory / "events.jsonl", "w") as handle:
        for frame in frames:
            handle.write(json.dumps(frame) + "\n")
    (directory / "team.json").write_text(json.dumps({"team": team, "messages": messages}, indent=1) + "\n")
    for name, log in helper_frames.items():
        with open(directory / f"events-helper-{re.sub(r'[^A-Za-z0-9_-]', '_', name)}.jsonl", "w") as handle:
            for frame in log:
                handle.write(json.dumps(frame) + "\n")
    copy = directory / "workspace"
    for path, data in files:
        target = copy / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    copy.mkdir(exist_ok=True)
    text = answer(frames)
    (directory / "answer.md").write_text(text)
    changed = changed_files(task.directory / "seed", files)
    (directory / "changed.json").write_text(json.dumps(changed, indent=1, sort_keys=True) + "\n")
    names = [helper["name"] for helper in (team or {}).get("helpers", [])]
    names += sorted({message["to"]["name"] for message in messages
                     if message["type"] == "assign" and message["to"].get("name")} - set(names))
    roles = sorted({helper["role"] for helper in (team or {}).get("helpers", []) if helper.get("role")})
    if team is not None:
        ended = [message for message in messages if message["type"] == "ended"]
        if ended and ended[-1].get("tokens", {}).get("members"):
            # The end note's per-member tokens are the relay's final count; the live record may trail it.
            members = ended[-1]["tokens"]["members"]
            followed.helper_tokens = sum(member["tokens"] for member in members)
            names += sorted({member["name"] for member in members} - set(names))
    # The final count decides, as the live one does, on both sides: a team whose helpers overspent between two polls
    # is over its budget even when its lead then completed.
    if followed.lead_tokens + followed.helper_tokens > task.max_tokens:
        followed.over_budget = True
    ending, code = terminal(frames)
    completed = ending == "run.completed" and not followed.over_budget and not followed.deadline_passed
    verdict = settings.check(task, copy, directory / "check") if completed and followed.problem is None else None
    outcome = (verdict or {}).get("outcome")
    rerun = rerun_reason(followed, frames, helper_frames.values())
    # The harness's or the model server's, not the model's: listed, never counted (a rerun reason reruns it).
    passed = None if outcome == "broken" or followed.problem or rerun else outcome == "passed"
    # The evidence is saved: the task leaves the server now (its workspace and, on the team's side, the helpers').
    if followed.conversation:
        for remove, target in ((api.delete_conversation, followed.conversation), (api.delete_workspace, workspace)):
            try:
                remove(target)
            except ApiError:
                pass  # left for tqa-e2e's sweep; a failed task delete never keeps the workspace
    return {**line, "conversation": followed.conversation, "ending": ending, "code": code,
            "check": outcome, "check_note": (verdict or {}).get("note"), "passed": passed,
            "failures": failures(followed, frames, team, messages, side),
            "tokens": followed.lead_tokens + followed.helper_tokens, "lead_tokens": followed.lead_tokens,
            "steps": followed.steps, "runs": followed.runs, "wall_secs": round(followed.wall_secs, 1),
            "over_budget": followed.over_budget, "deadline_passed": followed.deadline_passed,
            "helpers": len(names), "helper_names": names, "helper_roles": roles, "team_messages": len(messages),
            "stop_reason": (team or {}).get("stop_reason"), "problem": followed.problem, "rerun": rerun}


def done_attempts(lines):
    """From a stopped run's lines: the attempts done (rerun or not needed) and those whose owed rerun never ran, by
    (task, side, number)."""
    ran = {(line["task"], line["side"], str(line["attempt"])) for line in lines}
    done, owed = set(), set()
    for line in lines:
        if line.get("rerun_of") is not None:
            continue
        key = (line["task"], line["side"], line["attempt"])
        if line.get("rerun") and (line["task"], line["side"], f"{line['attempt']}r") not in ran:
            owed.add(key)
        else:
            done.add(key)
    return done, owed


def read_lines(out):
    """A stopped run's attempts.jsonl as lines. A half-written last line (a stop mid-write) moves to
    attempts.partial and leaves the file ending with its last whole line, so appending goes on cleanly."""
    path = Path(out) / "attempts.jsonl"
    if not path.exists():
        return []
    text = path.read_text()
    whole, _, rest = text.rpartition("\n")
    if rest:
        with open(Path(out) / "attempts.partial", "a") as partial:
            partial.write(rest + "\n")
        path.write_text(whole + "\n" if whole else "")
    return [json.loads(line) for line in whole.splitlines() if line.strip()]


def set_aside(out, task, side, number, interrupted=None):
    """An attempt cut off before its line: its partial evidence moves to `<attempt>.interrupted-<n>`, listed."""
    directory = Path(out) / "attempts" / f"{task.id}-{side}-{number}"
    if not directory.exists():
        return
    n = 1
    while directory.with_name(f"{directory.name}.interrupted-{n}").exists():
        n += 1
    kept = directory.with_name(f"{directory.name}.interrupted-{n}")
    directory.rename(kept)
    if interrupted is not None:
        interrupted.write(json.dumps({"task": task.id, "side": side, "attempt": number, "kept": kept.name}) + "\n")
        interrupted.flush()


def stop_time(text, now):
    """The epoch time of the next `HH:MM`, local time, after `now`: 07:40 at 23:00 is the next morning's."""
    match = re.fullmatch(r"([01]\d|2[0-3]):([0-5]\d)", text)
    if not match:
        raise SystemExit(f"--stop-at takes HH:MM (24 h, local time), not {text!r}")
    day = time.localtime(now)
    at = time.mktime((day.tm_year, day.tm_mon, day.tm_mday, int(match[1]), int(match[2]), 0, 0, 0, -1))
    if at <= now:
        at = time.mktime((day.tm_year, day.tm_mon, day.tm_mday + 1, int(match[1]), int(match[2]), 0, 0, 0, -1))
    return at


def run_settings(arguments):
    """A run's settings as `run.json` keeps them and a resume compares them: the arguments but SETTINGS_IGNORED."""
    return {key: value for key, value in arguments.items() if key not in SETTINGS_IGNORED}


def run_all(tasks, out, settings, repeat=REPEAT, attempt=None, log=None, switch_log=None, resumed=(),
            interrupted=None, stop_at=None, sides=SIDES):
    """Every attempt in the design's order, the sides switched only when the side changes (twice a task: the
    per-task interleaving guards against the model server drifting over the run). Each switch's start and length go
    to `switch_log`, for the report. An attempt with a rerun reason runs once more as `<n>r`, and nothing else is
    rerun; both runs are kept and listed. With `resumed` (a stopped run's lines), what they show done is skipped.
    With `stop_at` (an epoch time), no attempt or rerun starts whose wall budget plus STOP_MARGIN_SECS would end
    after it: the run stops there, and a resume goes on from the first one not run (a rerun stays owed). Only the
    `sides` given run."""
    attempt = attempt or run_attempt
    lines = list(resumed)
    done, owed = done_attempts(lines)

    def record(runs):
        for line in runs:
            lines.append(line)
            if log is not None:
                log.write(json.dumps(line) + "\n")
                log.flush()
            print(f"{line['task']} {line['side']} #{line['attempt']}: passed={line['passed']} "
                  f"failures={line['failures']}", flush=True)
    current = None
    def fits(task, number):
        if stop_at is None or settings.now() + task.wall_clock_secs + STOP_MARGIN_SECS <= stop_at:
            return True
        print(f"cutoff: {task.id} #{number} ({task.wall_clock_secs} s) would end after the stop; stopping here, "
              "--resume goes on", flush=True)
        return False

    for task, side, number in order(tasks, repeat, sides):
        if (task.id, side, number) in done:
            continue
        if not fits(task, number):
            return lines
        if side != current:
            at, began = settings.wall_time(), settings.clock()
            settings.switch(side)
            current = side
            if switch_log is not None:
                switch_log.write(json.dumps({"before": task.id, "side": side, "at": at,
                                             "secs": round(settings.clock() - began, 1)}) + "\n")
                switch_log.flush()
        if (task.id, side, number) in owed:
            runs = []
        else:
            set_aside(out, task, side, number, interrupted)
            settings.prepare(task, side, number)
            runs = [attempt(task, side, number, out, settings)]
        if (task.id, side, number) in owed or runs[0].get("rerun"):
            if not fits(task, f"{number}r"):
                record(runs)
                return lines
            set_aside(out, task, side, f"{number}r", interrupted)
            settings.prepare(task, side, f"{number}r")
            again = attempt(task, side, f"{number}r", out, settings)
            again["rerun_of"] = number
            runs.append(again)
        record(runs)
    return lines


def final(lines):
    """Each attempt's last run: a rerun replaces the run it repeats."""
    superseded = {(line["task"], line["side"], line["rerun_of"]) for line in lines if line.get("rerun_of")}
    return [line for line in lines if (line["task"], line["side"], line["attempt"]) not in superseded]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tasks", type=Path, default=Path(__file__).resolve().parent / "tasks")
    parser.add_argument("--only", action="append", default=[], help="run only this tagged task (repeatable)")
    parser.add_argument("--repeat", type=int, default=REPEAT)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--route", required=True, help="the model route both sides use")
    parser.add_argument("--storage-root", type=Path, required=True, help="the gateway's host storage root")
    parser.add_argument("--tqa-e2e", required=True, help="the tqa-e2e binary, for --check-only")
    parser.add_argument("--runtime-image", required=True)
    parser.add_argument("--workstation-image")
    parser.add_argument("--resume", action="store_true",
                        help="go on with a stopped run in --out, with the same settings (module docstring)")
    parser.add_argument("--stop-at", help="HH:MM, local time: no attempt starts whose wall budget plus 5 min would "
                        "end after it; a resume goes on (not a setting a resume compares)")
    parser.add_argument("--side", action="append", choices=SIDES,
                        help="run only this side (repeatable; both when not given)")
    parser.add_argument("--switch", required=True,
                        help="a command run with `single` or `team` appended: restarts the gateway with "
                             "[agent_teams] off or on, and returns once it answers")
    args = parser.parse_args(argv)
    # Checked before anything is written, so a mistyped stop leaves no run behind.
    stop_at = stop_time(args.stop_at, time.time()) if args.stop_at else None
    environment = run_settings(vars(args))
    environment = json.loads(json.dumps(environment, default=str))
    environment["team_brief"] = TEAM_BRIEF
    if args.resume:
        try:
            stored = json.loads((args.out / "run.json").read_text())
        except (OSError, ValueError) as error:
            raise SystemExit(f"{args.out} has no run to resume: {error}") from None
        changed = sorted(key for key in set(stored) | set(environment) if stored.get(key) != environment.get(key))
        if changed:
            raise SystemExit(f"the run in {args.out} had other settings: {', '.join(changed)}")
    elif args.out.exists():
        raise SystemExit(f"{args.out} exists; a run is never overwritten (--resume goes on with a stopped one)")
    tasks = select_tasks(args.tasks, args.only)
    binary = str(Path(args.tqa_e2e).resolve())
    storage = Path(args.storage_root).absolute()

    def seed(task, workspace):
        command = ["docker", *seed_args(args.runtime_image, (task.directory / "seed").absolute(),
                                        storage / "workspaces" / workspace)]
        subprocess.run(command, check=True, capture_output=True, timeout=600)

    def check(task, workspace, record):
        command = [binary, "--check-only", "--tasks", str(args.tasks), "--task", task.id, "--workspace",
                   str(workspace), "--out", str(record), "--runtime-image", args.runtime_image]
        if args.workstation_image:
            command += ["--workstation-image", args.workstation_image]
        subprocess.run(command, check=True)
        return json.loads((Path(record) / "check.json").read_text())

    def switch(side):
        subprocess.run([*args.switch.split(), side], check=True, timeout=600)

    settings = Settings(Api(args.base_url, args.token_file), args.route, seed, check, switch)
    resumed = []
    if args.resume:
        resumed = read_lines(args.out)
        print(f"resuming {args.out}: {len(resumed)} lines done", flush=True)
    else:
        args.out.mkdir(parents=True)
        (args.out / "run.json").write_text(json.dumps(environment, indent=1) + "\n")
    with (open(args.out / "attempts.jsonl", "a") as log, open(args.out / "switches.jsonl", "a") as switch_log,
          open(args.out / "interrupted.jsonl", "a") as interrupted):
        lines = run_all(tasks, args.out, settings, args.repeat, log=log, switch_log=switch_log, resumed=resumed,
                        interrupted=interrupted, stop_at=stop_at, sides=tuple(args.side or SIDES))
    reruns = [line for line in lines if line.get("rerun_of")]
    print(f"{len(final(lines))} attempts, {len(reruns)} rerun: "
          + ", ".join(f"{line['task']} {line['side']} #{line['rerun_of']}" for line in reruns))
    return 0


if __name__ == "__main__":
    sys.exit(main())
