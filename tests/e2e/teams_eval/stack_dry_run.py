"""The agent-teams evaluation on the scripted model, before GPU time: every task once per side through runner.py on an
owned stack, so the runner, both arms and the switch between them are shaken out (the lead's request of 5 Oct 2026).

The stack is tests/e2e/dry_run.py's (the gateway and the scripted model, compose files and helpers reused). Before each
attempt the scripted model serves the task's dry-run/<side>.json; the arms switch by rewriting the gateway's
[agent_teams] section and restarting the gateway. It passes when every attempt passes its check, every team attempt
started its helper, took its report and ended by `done` with no failure class, no single attempt has a team, and the
packets build. Needs TQA_DOCKER_TESTS=1 and the built images; run it on its own, after the soak.

    TQA_DOCKER_TESTS=1 python3 tests/e2e/teams_eval/stack_dry_run.py [--only r1-incident-causes] [--logs DIR]
"""

import argparse
import importlib.util
import json
import os
import socket
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(HERE))
import report  # noqa: E402
import runner  # noqa: E402
from api import Api  # noqa: E402

spec = importlib.util.spec_from_file_location("e2e_dry_run", ROOT / "tests" / "e2e" / "dry_run.py")
e2e = importlib.util.module_from_spec(spec)
spec.loader.exec_module(e2e)
compose = e2e.compose
TEAMS_ON = "\n[agent_teams]\nenabled = true\n"


def verdicts(lines, tasks, switches):
    """What a passing dry run must show, as a list of problems (empty when it passed)."""
    problems = []
    final = runner.final(lines)
    for line in final:
        name = f"{line['task']} {line['side']}"
        if line.get("passed") is not True:
            problems.append(f"{name}: not passed ({line.get('check')}, {line.get('problem')})")
        if line.get("failures"):
            problems.append(f"{name}: failure classes {line['failures']}")
        if line["side"] == "team" and (line.get("helpers") != 1 or line.get("stop_reason") != "done"):
            problems.append(f"{name}: {line.get('helpers')} helper(s), ended by {line.get('stop_reason')}")
        if line["side"] == "single" and line.get("helpers"):
            problems.append(f"{name}: a single agent has a team")
    if len(final) != 2 * len(tasks):
        problems.append(f"{len(final)} attempts, not {2 * len(tasks)}")
    if len(switches) != 2 * len(tasks):
        problems.append(f"{len(switches)} switches, not {2 * len(tasks)}")
    return problems


class Stack:
    """The owned stack's arms and scripted sessions."""

    def __init__(self, env, directory, scenario):
        self.env, self.directory, self.scenario = env, directory, scenario
        # The stack is up, so the installation belongs to the container's user: take it back for the read, as switch()
        # does for its write (it never ran past this before: PermissionError on gateway.toml).
        compose.ownership(directory, env)
        self.base_config = (directory / "gateway.toml").read_text()
        compose.ownership(directory, env, "1000:1000")

    def switch(self, side):
        compose.ownership(self.directory, self.env)
        compose.write(self.directory / "gateway.toml", self.base_config + (TEAMS_ON if side == "team" else ""))
        compose.ownership(self.directory, self.env, "1000:1000")
        compose.command(e2e.COMPOSE + ["restart", "gateway"], self.env)
        compose.command(e2e.COMPOSE + ["up", "-d", "--no-build", "--no-deps", "--wait", "--wait-timeout", "90",
                                       "gateway"], self.env)

    def prepare(self, task, side, number):
        e2e.serve(self.env, self.scenario, task.directory / "dry-run" / f"{side}.json")


def run(env, root, directory, scenario, origin, images, tasks, tqa_e2e):
    token = root / "token"
    e2e.api_token(env, origin, token)
    stack = Stack(env, directory, scenario)
    storage = Path(env["TQA_STORAGE_ROOT"]).absolute()

    def seed(task, workspace):
        subprocess.run(["docker", *runner.seed_args(images["runtime"], (task.directory / "seed").absolute(),
                                                    storage / "workspaces" / workspace)],
                       check=True, capture_output=True, timeout=600, env=env)

    def check(task, workspace, record):
        subprocess.run([*tqa_e2e, "--check-only", "--tasks", str(HERE / "tasks"), "--task", task.id, "--workspace",
                        str(workspace), "--out", str(record), "--runtime-image", images["runtime"]],
                       cwd=ROOT, env=env, check=True, timeout=600)
        return json.loads((Path(record) / "check.json").read_text())

    settings = runner.Settings(Api(origin, token), "local", seed, check, stack.switch, prepare=stack.prepare)
    out = root / "run"
    out.mkdir()
    with open(out / "attempts.jsonl", "a") as log, open(out / "switches.jsonl", "a") as switch_log:
        lines = runner.run_all(tasks, out, settings, repeat=1, log=log, switch_log=switch_log)
    switches = report.load_switches(out)
    problems = verdicts(lines, tasks, switches)
    order = report.make_packets(out, root / "judging", tasks)
    if len(order) != len(runner.final(lines)):
        problems.append(f"{len(order)} packets for {len(runner.final(lines))} attempts")
    return out, problems


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--only", action="append", default=[], help="run only this task (repeatable)")
    parser.add_argument("--tqa-e2e", help="a built tqa-e2e binary (default: cargo run)")
    parser.add_argument("--keep", type=Path, help="copy the run and the packets here before teardown")
    parser.add_argument("--logs", type=Path, help="save the redacted gateway and scripted-model logs here")
    args = parser.parse_args()
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    tasks = runner.select_tasks(HERE / "tasks", args.only)
    tqa_e2e = ([str(Path(args.tqa_e2e).resolve())] if args.tqa_e2e
               else ["cargo", "run", "-q", "-p", "tqa-testkit", "--bin", "tqa-e2e", "--"])
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-teams-eval-")
    root = Path(temporary.name).resolve()
    directory, failure = root / "installation", None
    try:
        directory.mkdir(mode=0o700)
        (root / "scenario").mkdir(mode=0o755)
        scenario = root / "scenario" / "current.json"
        scenario.write_text((tasks[0].directory / "dry-run" / "single.json").read_text())
        scenario.chmod(0o644)
        env["TQA_E2E_SCENARIO"] = str(scenario)
        images = {"runtime": e2e.image_id(env, env.get("TQA_RUNTIME_IMAGE", "tqa-runtime:base"))}
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
            compose.prepare(directory, env, runtime=images["runtime"], port=port)
        images["workstation"] = e2e.image_id(env, env.get("TQA_WORKSTATION_IMAGE", "tqa-runtime:workstation"))
        compose.ownership(directory, env)
        models = directory / "models.toml"
        compose.write(models, models.read_text() + "context_window=131072\nstream_usage=true\n")
        e2e.configure_templates(directory / "gateway.toml", images)
        compose.ownership(directory, env, "1000:1000")
        compose.command(e2e.COMPOSE + ["up", "-d", "--no-build", "--wait", "--wait-timeout", "90"], env)
        out, problems = run(env, root, directory, scenario, f"http://127.0.0.1:{port}", images, tasks, tqa_e2e)
        if args.keep:
            subprocess.run(["cp", "-a", str(out), str(root / "judging"), str(args.keep)], check=True)
        if problems:
            raise AssertionError("the dry run found problems:\n" + "\n".join(problems))
        print(f"teams-eval dry run: {len(tasks)} task(s), both sides passed, the team ran and ended by done",
              flush=True)
    except BaseException as error:
        failure = error
        if not isinstance(error, subprocess.SubprocessError):
            print("teams-eval dry run failed: " + compose.redact(str(error), env), file=sys.stderr)
    finally:
        if args.logs:
            e2e.save_logs(env, args.logs)
        failure = compose.retire(env, failure, configuration=e2e.COMPOSE)
        compose.cleanup(directory, env, temporary, failure, compose=e2e.COMPOSE)


if __name__ == "__main__":
    try:
        main()
    except (OSError, subprocess.SubprocessError):
        raise SystemExit("teams-eval dry run failed; see diagnostics above") from None
