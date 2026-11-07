"""L7 short task titles on the real stack: a task the model names after its first completed run, and one the person
renamed first, which the gateway must never rename. The browser (titles-stack.spec.ts) reads both tasks back from the
gateway's API into the result."""

import json
from pathlib import Path

import compose

# The first message of each task names its script (the scripted model's prompt selection, TK-U-036).
NAMED = "Clean up sales.csv"
RENAMED = "Summarise the quarterly numbers for the board"
# The fake model's summary reply, served to the gateway's tool-less title call.
TITLE = "Sales CSV cleanup"
# The person's own title for the second task, set before its first message.
PERSON_TITLE = "Board summary"


def configure(directory, env):
    """Short titles on for this suite only: the base installation turns them off, so no other suite's scripted
    replies meet the title call."""
    compose.ownership(directory, env)
    config = directory / "gateway.toml"
    text = config.read_text()
    if "[tasks]" in text:
        text = text.replace("auto_title=false", "auto_title=true")
    else:
        text += "\n[tasks]\nauto_title=true\n"
    compose.write(config, text)
    compose.ownership(directory, env, "1000:1000")


def verify_overlay(env, configuration):
    """The scripted model may read only the declared titles scenario, read-only."""
    config = json.loads(compose.command(configuration + ["config", "--format", "json"], env, capture=True).stdout)
    mounts = config["services"]["fake-model"].get("volumes", [])
    assert len(mounts) == 1, "Unexpected model fixture mounts"
    mount = mounts[0]
    assert mount.get("type") == "bind" and mount.get("read_only") is True, "The scenario mount must be read-only"
    assert mount.get("target") == "/scenarios/titles.json", "Unexpected scenario target"
    assert mount.get("source") == str(Path("deploy/tests/fixtures/titles.json").resolve()), "Unexpected scenario source"


def verify_result(env):
    """The task the model named has its short title from the model; the one the person renamed keeps the person's
    title. Both as the gateway's API returned them to the browser."""
    result = Path(env["TQA_STACK_RESULT"])
    assert result.stat().st_size <= 16384, "Titles result exceeded its bound"
    record = json.loads(result.read_text())
    named, renamed = record["a"], record["b"]
    assert named["id"] and renamed["id"] and named["id"] != renamed["id"], "Two different tasks are expected"
    assert (named["title"], named["title_source"]) == (TITLE, "model"), \
        "The first task was not named by the model: {}".format((named["title"], named["title_source"]))
    assert (renamed["title"], renamed["title_source"]) == (PERSON_TITLE, "person"), \
        "The person's title was not kept: {}".format((renamed["title"], renamed["title_source"]))
    print("Titles: the model named the first task '{}' after its first run; the renamed task kept '{}'".format(
        TITLE, PERSON_TITLE), flush=True)
