"""E2E-AGENTS-020: an agent team end to end on the scripted model (docs/evals/agent-teams.md section 1).

The browser spec (web/e2e/agents-stack.spec.ts) drives a lead and its two helpers through fixtures/agents.json. Its
restart case asks for a gateway restart at a named step through a marker file: this side restarts the gateway's
container when `restart-wanted` appears and answers with `restarted` (or `restart-failed` and why). Each side waits
for the other within a bounded, scaled time and fails naming the marker it didn't get; nothing happens on a timer.
"""

import json
import os
import threading
import time
from pathlib import Path

import compose

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"

WANTED = "restart-wanted"
DONE = "restarted"
FAILED = "restart-failed"


def scale(env):
    """The harness's deadline scale (crates/tqa-testkit/src/deadline.rs): an integer from 1 to 20, else 1."""
    text = env.get("TQA_TEST_DEADLINE_SCALE", "").strip()
    return int(text) if text.isdigit() and 1 <= int(text) <= 20 else 1


def configure(directory, env):
    """Agent teams on (they are off by default); the rest of the gateway's file as the demo writes it."""
    compose.ownership(directory, env)
    config = directory / "gateway.toml"
    compose.write(config, config.read_text() + "\n[agent_teams]\nenabled = true\n")
    compose.ownership(directory, env, "1000:1000")


def verify_overlay(env, configuration):
    """The scripted model may read only the declared agents scenario, read-only."""
    config = json.loads(compose.command(configuration + ["config", "--format", "json"], env, capture=True).stdout)
    mounts = config["services"]["fake-model"].get("volumes", [])
    assert len(mounts) == 1, "Unexpected model fixture mounts"
    mount = mounts[0]
    assert mount.get("type") == "bind" and mount.get("read_only") is True, "The scenario mount must be read-only"
    assert mount.get("target") == "/scenarios/agents.json", "Unexpected scenario target"
    assert mount.get("source") == str((FIXTURES / "agents.json").resolve()), "Unexpected scenario source"


class Restarter:
    """Restarts the gateway each time the spec asks for it by writing `restart-wanted` in `markers`: E2E-AGENTS-020's
    run with a restart, then E2E-AGENTS-023's while helpers work. After each, `restarted` is written and the request
    removed, so the spec can ask again (it removes `restarted` first)."""

    def __init__(self, markers, configuration, env):
        self.markers = Path(markers)
        self.configuration = configuration
        self.env = env
        self.stop = threading.Event()
        self.outcome = None
        self.restarts = 0
        self.thread = threading.Thread(target=self.run, name="agents-restarter", daemon=True)

    def start(self):
        self.markers.mkdir(mode=0o700, exist_ok=True)
        self.thread.start()
        return self

    def run(self):
        # The whole browser run is the bound: the spec's own scaled wait fails first if the gateway never comes back.
        deadline = time.monotonic() + 1800 * scale(self.env)
        wanted = self.markers / WANTED
        while True:
            while not wanted.exists():
                if self.stop.is_set() or time.monotonic() > deadline:
                    if self.restarts == 0:
                        self.outcome = "the spec never wrote {}".format(WANTED)
                    return
                time.sleep(0.2)
            try:
                compose.command(self.configuration + ["restart", "gateway"], self.env, timeout=120)
                compose.command(self.configuration + ["up", "-d", "--no-build", "--wait", "--wait-timeout", "60",
                                                      "gateway"], self.env, timeout=120)
            except Exception as error:  # Reported to the spec, which fails naming it, and kept for the verdict.
                self.outcome = "the gateway's restart failed: {}".format(compose.redact(str(error), self.env))
                self.mark(FAILED, self.outcome)
                return
            wanted.unlink(missing_ok=True)
            self.restarts += 1
            self.mark(DONE, "ok")
            self.outcome = "restarted"

    def mark(self, name, text):
        """Writes the marker `name` whole or not at all: the spec waits for it to exist and then reads it, so a file
        created before its text is written would read empty."""
        partial = self.markers / (name + ".partial")
        partial.write_text(text)
        os.replace(partial, self.markers / name)

    def finish(self):
        self.stop.set()
        self.thread.join(timeout=30)
        return self.outcome


def verify_result(env, restarter):
    """Both runs of the script ended their team with one end note; the restart case restarted the gateway once."""
    assert restarter is not None and restarter.finish() == "restarted", "Restart case: {}".format(
        restarter.outcome if restarter else "no restarter")
    result = Path(env["TQA_STACK_RESULT"])
    assert result.stat().st_size <= 16384, "Agents result exceeded its bound"
    runs = json.loads(result.read_text())["runs"]
    assert [run["variant"] for run in runs] == ["plain", "restart"], "Unexpected runs: {}".format(runs)
    for run in runs:
        assert run["stop_reason"] == "done", "{}: the team ended as {}".format(run["variant"], run["stop_reason"])
        assert run["end_notes"] == 1, "{}: {} end notes".format(run["variant"], run["end_notes"])
        assert sorted(run["helpers"]) == ["analyst", "fixer"], "{}: helpers {}".format(run["variant"], run["helpers"])
    print("Agents: a lead and two helpers ran the script end to end, with and without a gateway restart", flush=True)


# What a failed run's team looked like in the database, beside the stack's logs: the teams, their members, the members'
# turns (wakes and first turns, with their status) and each conversation's run lifecycle. No message bodies.
TEAM_STATE = """
SELECT id, state, stop_reason, lead_last_read_seq FROM agent_teams;
SELECT name, workspace, state, last_read_seq, conversation_id FROM agent_team_members ORDER BY created_at;
SELECT i.conversation_id, split_part(i.key, ':', 1) AS kind, i.execution_status, i.held, i.created_at, i.updated_at
  FROM idempotency i WHERE i.conversation_id IN (SELECT conversation_id FROM agent_team_members
  UNION SELECT lead_conversation FROM agent_teams) ORDER BY i.ordinal;
SELECT conversation_id, seq, run_id, lifecycle, created_at FROM events WHERE lifecycle IS NOT NULL
  ORDER BY conversation_id, seq;
SELECT seq, type, to_kind, author_name, created_at FROM channel_messages ORDER BY seq;
SELECT conversation_id, phase, updated_at FROM sandbox_instances;
"""


def save_team_state(env, folder, configuration):
    """After a failed run, the team's stored state in `folder`/team-state.txt. Never raises: a failure to read it is
    reported and the run's own failure stands."""
    try:
        result = compose.command(configuration + ["exec", "-T", "postgres", "psql", "-U", "tqa", "-d", "gateway",
                                                  "-X", "-c", TEAM_STATE], env, capture=True)
        path = Path(folder) / "team-state.txt"
        compose.write(path, compose.redact(result.stdout, env))
        print("Kept the team's stored state: " + str(path), flush=True)
    except Exception as error:
        print("Could not keep the team's stored state: " + compose.redact(str(error), env), flush=True)
