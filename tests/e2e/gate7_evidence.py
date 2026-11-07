"""E2E-CHANNEL-016, the Gate 7 demo (DEV-PLAN Phase 7; ADR-0014): its script and checks, without a stack.

Two agents and a person on one channel. The agents are real runs in sandboxes, answered by the scripted fake model
(one fake-model service per agent, each with its own scenario, filled in with the channel's id once it exists). The
person acts from the channel page (WEB-E2E-012, web/e2e/channel-stack.spec.ts). The channel's rule is `majority`; the
person creates it and so leads it; the two agent conversations are members: three voting members.

Every step waits for the one it depends on (an agent's `channel_wait` filtered by type, the person's spec waiting on
the page), so each message's seq is fixed. That is what lets static scenarios name the seqs they vote on and claim:

  1 text      Planner   says it will propose
  2 proposal  Planner   approach A
  3 proposal  Builder   approach B (after seeing 2)
  4 vote      Builder   accept on 2, with a reason
  5 vote      person    accept on 2: two of three, so
  6 decision  gateway   2 accepted
  7 vote      person    veto on 3
  8 decision  gateway   3 vetoed
  9 task      Planner   the work, claims 2 (after both decisions)
 10 claim     Planner   the winner claims 9
 11 report    Planner   completed
 12 claim     person    the second place, from the board
 13 report    person    completed
 14 done      Planner   (after both reports)

The Planner then answers and its run completes; the Builder's run completes after it sees its proposal vetoed (8),
without posting. Expected evidence (every item asserted; the run fails on the first miss):
- C1 the log: exactly the fourteen messages above, contiguous seqs, each type, author kind and name, and body as
  scripted; decisions written by the gateway.
- C2 decisions: 2 `accepted` under `majority`, 3 `vetoed`; the proposal states agree (tally 2 accept, 0 reject for 2).
- C3 claims: task 9 takes two claimants, the Planner's and the person's, and each reported `completed`; the board
  says so.
- C4 the runs: both agent runs completed; each agent's tool calls were only channel tools, in the scripted order; the
  Builder never claimed; no run failed.
- C5 the page (the spec's result file): the seqs it saw match the log.
"""

import json

CHANNEL_NAME = "gate7-demo"
PERSON_EMAIL = "channels@example.invalid"
AGENTS = ("Planner", "Builder")
ROUTES = {"Planner": "planner", "Builder": "builder"}
CHANNEL_TOOLS = ["channel_post", "channel_read", "channel_wait", "task_claim"]

SCRIPT = {
    "agents": list(AGENTS),
    "hello": "I will propose how to parse the release notes.",
    "proposal": "Approach A: a streaming parser",
    "proposal_text": "Read the notes line by line; no buffering of the whole file.",
    "counter": "Approach B: one regular expression",
    "counter_text": "A single pattern over the whole file.",
    "reason": "A is easier to test.",
    "veto": "Approach B: one regular expression",
    "task": "Parse the release notes",
    "spec": "Implement approach A and report the section count.",
    "report": "Parsed 4 sections with the streaming parser.",
    "person_report": "Checked the output by hand: 4 sections.",
    "done": "Both reports are in; the work is done.",
}

# (seq, type, author: "Planner" | "Builder" | "person" | "gateway", fields of the body that must match)
LOG = [
    (1, "text", "Planner", {"text": SCRIPT["hello"]}),
    (2, "proposal", "Planner", {"title": SCRIPT["proposal"], "text": SCRIPT["proposal_text"]}),
    (3, "proposal", "Builder", {"title": SCRIPT["counter"], "text": SCRIPT["counter_text"]}),
    (4, "vote", "Builder", {"proposal_seq": 2, "choice": "accept", "reason": SCRIPT["reason"]}),
    (5, "vote", "person", {"proposal_seq": 2, "choice": "accept"}),
    (6, "decision", "gateway", {"proposal_seq": 2, "outcome": "accepted", "rule": "majority"}),
    (7, "vote", "person", {"proposal_seq": 3, "choice": "veto"}),
    (8, "decision", "gateway", {"proposal_seq": 3, "outcome": "vetoed", "rule": "majority"}),
    (9, "task", "Planner", {"title": SCRIPT["task"], "spec": SCRIPT["spec"], "claims": 2}),
    (10, "claim", "Planner", {"task_seq": 9}),
    (11, "report", "Planner", {"task_seq": 9, "status": "completed", "summary": SCRIPT["report"]}),
    (12, "claim", "person", {"task_seq": 9}),
    (13, "report", "person", {"task_seq": 9, "status": "completed", "summary": SCRIPT["person_report"]}),
    (14, "done", "Planner", {"summary": SCRIPT["done"]}),
]

# Each agent's channel tool calls, in order, as (tool, the arguments that must match). The fake-model scenarios make
# exactly these calls; the driver checks the runs' events against them.
CALLS = {
    "Planner": [
        ("channel_post", {"type": "text"}),
        ("channel_post", {"type": "proposal"}),
        ("channel_wait", {"types": ["decision"]}),
        ("channel_wait", {"types": ["decision"]}),
        ("channel_post", {"type": "task"}),
        ("task_claim", {"task_seq": 9}),
        ("channel_post", {"type": "report"}),
        ("channel_wait", {"types": ["report"]}),
        ("channel_post", {"type": "done"}),
    ],
    "Builder": [
        ("channel_wait", {"types": ["proposal"]}),
        ("channel_post", {"type": "proposal"}),
        ("channel_post", {"type": "vote"}),
        ("channel_wait", {"types": ["decision"]}),
        ("channel_wait", {"types": ["decision"]}),
    ],
}


def script_file():
    """The words the agents and the person post, for the page spec (TQA_GATE7_SCRIPT)."""
    return json.dumps(SCRIPT, sort_keys=True)


def profile(agent):
    """An agent's profile on its own fake model's route. It names no tool allow-list: the gateway checks a profile's
    list against the built-in tool names, which don't include the channel tools yet, and the runtime registers the
    channel tools for a member conversation anyway. C4 checks that each agent called only channel tools."""
    return {"name": agent.lower(), "display_name": agent, "model_route": ROUTES[agent],
            "instructions": "You are {} on the team channel.".format(agent), "skills": "none", "mcp_servers": [],
            "connectors": []}


def page_files(installation):
    """The page spec's script and result files: beside the installation, in the host user's temporary root, since the
    installation belongs to the gateway user by the time the spec runs (and Playwright writes the result)."""
    return installation.parent / "gate7-script.json", installation.parent / "channel-page.json"


def scenario_writer(path, image):
    """The command that rewrites a fake model's scenario file after the installation belongs to the gateway user
    (1000:1000): a networkless container, running as that user, that sees only the one file and copies its input
    into it. The host never needs to write there."""
    return ["docker", "run", "--rm", "-i", "--network", "none", "--user", "1000:1000", "--mount",
            "type=bind,source={},target=/scenario.json".format(path), image, "tee", "/scenario.json"]


def refusal(path, status, body):
    """A refused request as one line: the path, the status and the start of the error body (which names the field)."""
    return "{} answered {}: {}".format(path, status, body[:2048].decode("utf-8", errors="replace"))


def author_of(message, names):
    """The scripted author of a stored message: an agent's name, `person` or `gateway`."""
    author = message["author"]
    if author["kind"] == "gateway":
        return "gateway"
    if author["kind"] == "user":
        return "person"
    return names.get(author["id"], "conversation " + author["id"])


def check_log(messages, names):
    """C1: exactly the scripted log. `names` maps conversation ids to agent names."""
    got = [(message["seq"], message["body"]["type"], author_of(message, names)) for message in messages]
    want = [(seq, kind, author) for seq, kind, author, _ in LOG]
    assert got == want, "the log differs from the script:\n got  {}\n want {}".format(got, want)
    for message, (seq, _, _, fields) in zip(messages, LOG):
        for key, value in fields.items():
            assert message["body"].get(key) == value, "seq {} {}: {!r} != {!r}".format(seq, key,
                                                                                        message["body"].get(key), value)
    return {"messages": len(messages)}


def check_decisions(accepted, vetoed):
    """C2: the proposal states of 2 (accepted, two of three accepting) and 3 (vetoed)."""
    assert accepted["state"] == "decided" and accepted["decision"]["outcome"] == "accepted", accepted
    assert accepted["decision"]["seq"] == 6 and accepted["tally"] == {"accept": 2, "reject": 0}, accepted
    assert accepted["voting_members"] == 3, accepted
    assert vetoed["decision"] == {"seq": 8, "outcome": "vetoed"}, vetoed
    return {"accepted": 2, "vetoed": 3}


def check_board(board, names):
    """C3: task 9 with two claims (the Planner's, the person's), each reported completed."""
    tasks = board["items"]
    assert [task["task_seq"] for task in tasks] == [9], board
    task = tasks[0]
    assert task["limit"] == 2 and task["title"] == SCRIPT["task"], task
    claimants = [author_of({"author": claim["author"]}, names) for claim in task["claims"]]
    assert claimants == ["Planner", "person"], claimants
    reports = [(author_of({"author": report["author"]}, names), report["status"]) for report in task["reports"]]
    assert reports == [("Planner", "completed"), ("person", "completed")], reports
    return {"claims": 2, "reports": 2}


def tool_calls(events):
    """The (tool, arguments) of every tool call a run's events record, in order."""
    calls = []
    for frame in events:
        event = frame.get("event") or {}
        if event.get("type") == "tool.call":
            arguments = event.get("arguments")
            if isinstance(arguments, str):
                arguments = json.loads(arguments)
            calls.append((event.get("name"), arguments or {}))
    return calls


def check_run(agent, events):
    """C4: the agent's run completed, with only the scripted channel tool calls."""
    types = [(frame.get("event") or {}).get("type") for frame in events]
    assert "run.completed" in types, "{}'s run did not complete: {}".format(agent, types)
    assert "run.failed" not in types, "{}'s run failed".format(agent)
    calls = tool_calls(events)
    assert all(name in CHANNEL_TOOLS for name, _ in calls), "{} called other tools: {}".format(agent, calls)
    want = CALLS[agent]
    assert [name for name, _ in calls] == [name for name, _ in want], "{}'s calls: {}".format(agent, calls)
    for (name, arguments), (_, expected) in zip(calls, want):
        for key, value in expected.items():
            got = arguments.get(key, (arguments.get("body") or {}).get(key))
            assert got == value, "{} {}: {} is {!r}, expected {!r}".format(agent, name, key, got, value)
    return {agent: len(calls)}


def check_page(result):
    """C5: what the page spec saw, against the log."""
    want = {"proposal_seq": 2, "decision_seq": 6, "veto_seq": 7, "task_seq": 9}
    got = {key: result.get(key) for key in want}
    assert got == want, "the page saw {}, the log says {}".format(got, want)
    order = result.get("order", [])
    assert order == sorted(order) and order == list(range(1, len(order) + 1)), order
    return {"page_seen": len(order)}
