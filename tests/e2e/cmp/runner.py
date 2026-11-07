"""E2E-CMP-003: the tagged tasks on the reference product, with the same model server, task statements, step limits,
wall clocks, seeds and checks as our own attempts, one attempt at a time. See README.md for the method.

Each attempt:
1. seeds a workspace with `tqa-e2e --seed-only` (the driver's own seeding, so the same baseline commit);
2. starts the reference agent server in a container on the model's internal network only, with the workspace
   mounted, and a forwarder of ours that publishes it on loopback (the reference itself gets no other network);
3. starts one conversation with `task.md` verbatim and the task's step limit, and polls it until it ends or the task's
   wall clock plus the margin runs out;
4. always removes both containers, then scores the workspace with `tqa-e2e --check-only`;
   attempts run in tqa-e2e's order: each task's attempts together, tasks in id order;
5. appends one line to attempts.jsonl.

The model key is read from a mode-0600 file, sent only in the start request, and removed from everything saved.
"""

import argparse
import json
import math
import secrets
import stat
import subprocess
import sys
import time
import tomllib
import urllib.error
import urllib.request
from pathlib import Path
from typing import Callable, NamedTuple, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
import reference  # noqa: E402

READY_SECS = 180
# Our forwarder: publishes the reference's API on loopback from a container on both networks, as the local-model
# forwarder does. Standard library only, bounded to the attempt by its removal.
FORWARD_SCRIPT = """
import asyncio, sys
TARGET, PORT = sys.argv[1], int(sys.argv[2])
async def pipe(reader, writer):
    try:
        while data := await reader.read(65536):
            writer.write(data)
            await writer.drain()
    finally:
        writer.close()
async def handle(client_reader, client_writer):
    try:
        upstream_reader, upstream_writer = await asyncio.wait_for(asyncio.open_connection(TARGET, PORT), 10)
    except Exception:
        client_writer.close()
        return
    await asyncio.gather(pipe(client_reader, upstream_writer), pipe(upstream_reader, client_writer),
                         return_exceptions=True)
async def main():
    server = await asyncio.start_server(handle, "0.0.0.0", 8000)
    async with server:
        await server.serve_forever()
asyncio.run(main())
"""


class Task(NamedTuple):
    id: str
    text: str
    max_steps: int
    wall_clock_secs: int
    template: str
    max_tokens: int = 0


class Settings(NamedTuple):
    reference_image: str
    workstation_reference_image: Optional[str]
    forward_image: str
    network: str
    model_url: str
    model: str
    model_key: str
    poll_secs: float
    margin_secs: float
    docker: Callable
    seed: Callable
    check: Callable
    reference_user: str = "0:0"
    # Our route's request settings, sent the same way to the reference (reference.start_body).
    max_output: Optional[int] = None
    extra_body: Optional[dict] = None


def select_tasks(root, tag, only=()):
    """The tasks carrying `tag`, in id order; `only` narrows them to those ids, each of which must carry the tag."""
    tasks = []
    for directory in sorted(Path(root).iterdir()):
        manifest_path = directory / "task.toml"
        if not manifest_path.is_file():
            continue
        manifest = tomllib.loads(manifest_path.read_text())
        if tag not in manifest.get("tags", []):
            continue
        budgets = manifest["budgets"]
        tasks.append(Task(manifest["id"], (directory / "task.md").read_text(), budgets["max_steps"],
                          budgets["wall_clock_secs"], manifest.get("template", "default"), budgets["max_tokens"]))
    if only:
        missing = sorted(set(only) - {task.id for task in tasks})
        if missing:
            raise SystemExit("--only names tasks not tagged {}: {}".format(tag, ", ".join(missing)))
        tasks = [task for task in tasks if task.id in only]
    return tasks


def require_pinned(image):
    digest = image.rsplit("@", 1)[-1] if "@" in image else image
    if not (digest.startswith("sha256:") and len(digest) == len("sha256:") + 64):
        raise SystemExit("the reference image must be pinned by digest or image ID: " + image)


def reference_args(name, image, workspace, network, session_key, user="0:0"):
    return ["run", "-d", "--name", name, "--network", network, "--user", user,
            "--cpus", "2", "--memory", "4g",
            "--mount", "type=bind,source={},target={}".format(workspace, reference.WORKDIR),
            "--env", "{}={}".format(reference.SESSION_ENV, session_key),
            image]


def forward_args(name, image, target):
    return ["run", "-d", "--name", name, "--read-only", "--cap-drop", "ALL", "-p", "127.0.0.1::8000",
            "--entrypoint", "python3", image, "-c", FORWARD_SCRIPT, target, str(reference.SERVER_PORT)]


def new_session_key():
    return secrets.token_hex(24)


def redact(value, hidden):
    text = json.dumps(value)
    for secret in hidden:
        if secret:
            text = text.replace(secret, "[redacted]")
    return json.loads(text)


def classify(status, outcome, stopped):
    """completed, model (wrong or incomplete work), harness (the product or the check broke), wall_clock (stopped at the
    task's wall clock) or setup (the attempt never started; not counted, rerun)."""
    if stopped:
        return "wall_clock"
    if status is None:
        return "setup"
    if outcome == "broken" or status == "error":
        return "harness"
    if outcome == "passed":
        return "completed"
    return "model"


def wilson(successes, total, z=1.96):
    if total == 0:
        return 0.0, 0.0
    phat = successes / total
    denominator = 1 + z * z / total
    centre = phat + z * z / (2 * total)
    spread = z * math.sqrt(phat * (1 - phat) / total + z * z / (4 * total * total))
    return max(0.0, (centre - spread) / denominator), min(1.0, (centre + spread) / denominator)


class Api:
    def __init__(self, base, session_key):
        self.base, self.session_key = base, session_key

    def call(self, method, path, body=None):
        headers = {reference.SESSION_HEADER: self.session_key, "Content-Type": "application/json"}
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(self.base + path, data=data, headers=headers, method=method)
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read() or b"null")

    def ready(self, deadline):
        while time.monotonic() < deadline:
            try:
                with urllib.request.urlopen(self.base + reference.HEALTH, timeout=5) as response:
                    if response.status == 200:
                        return True
            except (urllib.error.URLError, OSError):
                pass
            time.sleep(1)
        return False


def run_attempt(task, number, out, settings):
    name = "tqa-cmp-{}-{}".format(task.id, number)
    forwarder = "tqa-cmp-fwd-{}-{}".format(task.id, number)
    directory = Path(out) / "attempts" / "{}-{}".format(task.id, number)
    workspace = directory / "workspace"
    settings.seed(task, workspace)
    image = settings.workstation_reference_image if task.template == "workstation" else settings.reference_image
    status, info, stopped, budget_stop, problem, kind = None, None, False, False, None, None
    started = time.monotonic()
    try:
        if not image:
            raise RuntimeError("no reference image for template " + task.template)
        key = new_session_key()
        settings.docker(reference_args(name, image, workspace.resolve(), settings.network, key,
                                       settings.reference_user))
        settings.docker(forward_args(forwarder, settings.forward_image, name))
        settings.docker(["network", "connect", settings.network, forwarder])
        port = settings.docker(["port", forwarder, "8000/tcp"]).strip().rsplit(":", 1)[-1]
        api = Api("http://127.0.0.1:" + port, key)
        if not api.ready(time.monotonic() + READY_SECS):
            raise RuntimeError("the reference did not become ready")
        body = reference.start_body(task.text, settings.model_url, settings.model, settings.model_key, task.max_steps,
                                    settings.max_output, settings.extra_body)
        conversation = api.call("POST", reference.CONVERSATIONS, body)
        path = reference.conversation_path(conversation["id"])
        deadline = started + task.wall_clock_secs + settings.margin_secs
        while True:
            info = api.call("GET", path)
            status = info.get("execution_status")
            if reference.terminal(status):
                if status == "error":
                    kind = reference.error_kind(api.call("GET", reference.ERROR_EVENTS.format(conversation["id"])))
                break
            # The task's token budget, as our side's harness enforces it: a budget stop is the model's failure.
            used = reference.tokens(info)
            if task.max_tokens and used and used["prompt"] + used["completion"] > task.max_tokens:
                budget_stop = True
                break
            if time.monotonic() >= deadline:
                stopped = True
                break
            time.sleep(settings.poll_secs)
    except (RuntimeError, OSError, ValueError, KeyError, urllib.error.URLError) as error:
        problem = "{}: {}".format(type(error).__name__, error)
    finally:
        settings.docker(["rm", "-f", name, forwarder], check=False)
    wall_ms = int((time.monotonic() - started) * 1000)
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "conversation.json").write_text(
        json.dumps(redact(info, [settings.model_key]), indent=1) + "\n")
    started_ok = status is not None or stopped or budget_stop
    verdict = settings.check(task, workspace, directory / "check") if started_ok else {"outcome": None}
    # A budget stop, or an error the reference classes as the agent's own outcome (its step limit, a bad action), is
    # the model's failure, as for our side; any other error is the product's, and only a provider error is rerun.
    provider = status == "error" and kind in reference.PROVIDER_KINDS
    if budget_stop or (status == "error" and kind in reference.AGENT_KINDS):
        outcome_class = "model"
    else:
        outcome_class = classify(status if started_ok else None, verdict.get("outcome"), stopped)
    line = {"task": task.id, "attempt": number, "class": outcome_class,
            "outcome": verdict.get("outcome"), "status": status, "stopped": stopped, "budget_stop": budget_stop,
            "error_kind": kind, "provider": provider,
            "wall_ms": wall_ms,
            "tokens": reference.tokens(info), "check_note": verdict.get("note"), "dropped": verdict.get("dropped"),
            "problem": problem}
    return line


def order(tasks, repeat):
    """The attempts in tqa-e2e's order: each task's attempts together, tasks in id order, so both sides of the
    comparison meet the model server in the same sequence."""
    return [(task, number) for task in tasks for number in range(1, repeat + 1)]


def run_all(tasks, repeat, out, settings, attempt=None, log=None):
    """Every attempt in tqa-e2e's order. An attempt whose error the reference's own event log classes as the model
    server or the connection failing (`provider`) runs once more, as our harness reruns an attempt that failed with a
    provider error, and nothing else is rerun: completion then counts the rerun, while the failure still counts."""
    attempt = attempt or run_attempt
    lines = []
    for task, number in order(tasks, repeat):
        line = attempt(task, number, out, settings)
        runs = [line]
        if line.get("provider"):
            rerun = attempt(task, "{}r".format(number), out, settings)
            rerun["rerun_of"] = number
            runs.append(rerun)
        for run in runs:
            lines.append(run)
            if log is not None:
                log.write(json.dumps(run) + "\n")
                log.flush()
            print("{} #{}: {}".format(task.id, run["attempt"], run["class"]), flush=True)
    return lines


def final(lines):
    """Each attempt's last run: a rerun replaces the run it repeats."""
    superseded = {(line["task"], line["rerun_of"]) for line in lines if line.get("rerun_of") is not None}
    return [line for line in lines if (line["task"], line["attempt"]) not in superseded]


def summary(lines):
    harness = sum(1 for line in lines if line["class"] == "harness")
    lines = final(lines)
    counted = [line for line in lines if line["class"] != "setup"]
    completed = sum(1 for line in counted if line["class"] == "completed")
    low, high = wilson(completed, len(counted))
    setup = len(lines) - len(counted)
    out = ["# E2E-CMP-003, reference side", "",
           "**{}/{} completed** ({:.1f} %; 95 % Wilson {:.1f}–{:.1f} %)".format(
               completed, len(counted), 100 * completed / len(counted) if counted else 0, 100 * low, 100 * high)]
    out.append("{} harness failure{} (every run counted, reruns included)".format(harness, "" if harness == 1 else "s"))
    if setup:
        out.append("{} setup failure{} (not counted; rerun)".format(setup, "" if setup == 1 else "s"))
    classes = {}
    for line in counted:
        classes[line["class"]] = classes.get(line["class"], 0) + 1
    out += ["", "Classes: " + ", ".join("{} {}".format(name, count) for name, count in sorted(classes.items())),
            "", "| Task | Completed |", "|---|---|"]
    for task in sorted({line["task"] for line in counted}):
        mine = [line for line in counted if line["task"] == task]
        out.append("| {} | {}/{} |".format(task, sum(1 for line in mine if line["class"] == "completed"), len(mine)))
    return "\n".join(out) + "\n"


def docker(args, check=True):
    result = subprocess.run(["docker", *args], capture_output=True, text=True, timeout=600)
    if check and result.returncode != 0:
        raise RuntimeError("docker {} failed: {}".format(args[0], result.stderr.strip()[-300:]))
    return result.stdout


def read_private(path):
    path = Path(path)
    if stat.S_IMODE(path.stat().st_mode) & 0o077:
        raise SystemExit("{} must be mode 0600".format(path))
    return path.read_text().strip()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tasks", type=Path, default=Path(__file__).resolve().parent.parent / "tasks")
    parser.add_argument("--tag", default="cmp")
    parser.add_argument("--only", action="append", default=[], help="run only this tagged task (repeatable)")
    parser.add_argument("--repeat", type=int, default=5)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--tqa-e2e", required=True, help="the tqa-e2e binary, for --seed-only and --check-only")
    parser.add_argument("--runtime-image", required=True)
    parser.add_argument("--workstation-image")
    parser.add_argument("--reference-image", required=True, help="pinned by digest")
    parser.add_argument("--workstation-reference-image", help="the reference on the Workstation toolchain; pinned")
    parser.add_argument("--forward-image", required=True, help="a local image with python3, pinned by image ID")
    parser.add_argument("--network", default="tqa-a-model")
    parser.add_argument("--model-url", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--model-key-file", required=True)
    parser.add_argument("--margin-secs", type=float, default=60)
    parser.add_argument("--max-output", type=int, help="our route's max_output, sent the same to the reference")
    parser.add_argument("--extra-body", type=json.loads, default=None, help="our route's extra_body, as JSON")
    args = parser.parse_args(argv)
    for image in (args.reference_image, args.workstation_reference_image, args.forward_image):
        if image:
            require_pinned(image)
    if args.out.exists():
        raise SystemExit("{} exists; a run is never overwritten".format(args.out))
    tasks = select_tasks(args.tasks, args.tag, args.only)
    binary = str(Path(args.tqa_e2e).resolve())

    def seed(task, workspace):
        subprocess.run([binary, "--seed-only", "--tasks", str(args.tasks), "--task", task.id,
                        "--workspace", str(workspace), "--runtime-image", args.runtime_image], check=True)

    def check(task, workspace, record):
        command = [binary, "--check-only", "--tasks", str(args.tasks), "--task", task.id, "--workspace",
                   str(workspace), "--out", str(record), "--runtime-image", args.runtime_image]
        if args.workstation_image:
            command += ["--workstation-image", args.workstation_image]
        subprocess.run(command, check=True)
        return json.loads((Path(record) / "check.json").read_text())

    settings = Settings(args.reference_image, args.workstation_reference_image, args.forward_image, args.network,
                        args.model_url, args.model, read_private(args.model_key_file), 2.0, args.margin_secs,
                        docker, seed, check, max_output=args.max_output, extra_body=args.extra_body)
    args.out.mkdir(parents=True)
    environment = {key: value for key, value in vars(args).items() if key != "model_key_file"}
    (args.out / "run.json").write_text(json.dumps(environment, default=str, indent=1) + "\n")
    with open(args.out / "attempts.jsonl", "a") as log:
        lines = run_all(tasks, args.repeat, args.out, settings, log=log)
    (args.out / "summary.md").write_text(summary(lines))
    print(summary(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
