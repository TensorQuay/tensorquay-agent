"""TK-I-006: `tqa-e2e --concurrency 4` against `--concurrency 1` on an owned stack with the scripted model.

One task's scripted solution, four attempts, run once one at a time and once four at a time. Both runs must pass every
attempt, number them 1 to 4, give each its own conversation and workspace copy, and record the same attempts apart from
wall times. The task is a copy whose check first sleeps, so the four-at-a-time run must queue on the harness's limit:
sampled every 0.1 s, exactly two of its seed, check or hand-back containers run at once at the peak; the checks' own
start and end times never overlap more than two deep; and some attempt's check starts well after its copy was ready,
because it waited for a slot. The scripted model derives each reply from the request's own history, so concurrent
attempts of one task each get their script. Needs TQA_DOCKER_TESTS=1 and the built images; run it on its own, like
TK-I-001.
"""
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import threading

sys.path.insert(0, str(Path(__file__).resolve().parent))
# The stack set-up and the driver call are the dry run's.
from dry_run import COMPOSE, ROOT, TASKS, api_token, compose, configure_templates, driver, image_id  # noqa: E402

ATTEMPTS = 4
CONTAINERS = 2
# How long each check sleeps first, and how late a check must start after its copy was ready to count as waiting.
CHECK_SLEEP_SECS = 2
WAITED_SECS = 1.0
STAMP = "tqa-e2e-check-stamp"
# The copy's check: stamp, sleep, the task's own check, stamp, with the task's own exit status.
SLOW_CHECK = f"""set -u
stamp() {{ python3 -c 'import sys, time; print("{STAMP}", sys.argv[1], repr(time.time()))' "$1"; }}
stamp start
sleep {CHECK_SLEEP_SECS}
sh /check/task-check.sh
status=$?
stamp end
exit $status
"""


def slow_task(tasks, task):
    """A copy of `task` under `tasks` whose check sleeps first, readable by the check's own user."""
    copy = tasks / task
    shutil.copytree(TASKS / task, copy)
    check = copy / "check"
    (check / "run.sh").rename(check / "task-check.sh")
    (check / "run.sh").write_text(SLOW_CHECK)
    for path in [tasks, *tasks.rglob("*")]:
        path.chmod(0o755 if path.is_dir() else 0o644)


def harness_containers(env):
    """How many of the harness's own seed, check and hand-back containers run right now."""
    listed = compose.command(["docker", "ps", "-q", "--filter", "label=tqa.e2e.container=1"], env, capture=True)
    return len(listed.stdout.split())


def run_at(env, root, origin, images, token, tasks, task, concurrency, samples):
    out = root / "runs" / f"concurrency-{concurrency}"
    stop = threading.Event()

    def sample():
        while not stop.is_set():
            samples.append(harness_containers(env))
            stop.wait(0.1)

    sampler = threading.Thread(target=sample, daemon=True)
    sampler.start()
    try:
        run = driver(env, "--dry-run", "--base-url", origin, "--token-file", token, "--route", "local",
                     "--tasks", tasks, "--only", task, "--repeat", ATTEMPTS, "--concurrency", concurrency,
                     "--out", out, "--storage-root", env["TQA_STORAGE_ROOT"],
                     "--runtime-image", images["runtime"], "--gateway-image", images["gateway"],
                     "--workstation-image", images["workstation"], "--repository", ROOT)
    finally:
        stop.set()
        sampler.join(timeout=5)
    if run.returncode != 0:
        raise AssertionError(f"the driver at concurrency {concurrency} exited {run.returncode}")
    return [json.loads(line) for line in (out / "attempts.jsonl").read_text().splitlines()], out


def comparable(attempts):
    """Each attempt as the summary counts it, without its wall time, conversation or evidence position."""
    return sorted((attempt["attempt"], attempt["task_id"], attempt["class"], attempt["check"], attempt["rerun_of"])
                  for attempt in attempts)


def check_times(out):
    """Each attempt's copy: when it was ready for its check (its newest file) and the check's start and end stamps."""
    times = {}
    for copy in sorted(path for path in (out / "work").iterdir() if path.is_dir()):
        stamps = {}
        for line in (out / "work" / f"{copy.name}.check.txt").read_text().splitlines():
            words = line.split()
            if len(words) == 3 and words[0] == STAMP:
                stamps[words[1]] = float(words[2])
        if set(stamps) != {"start", "end"}:
            raise AssertionError(f"{copy.name}: the check's stamps are missing")
        ready = max(path.stat().st_mtime for path in [copy, *copy.rglob("*")])
        times[copy.name] = (ready, stamps["start"], stamps["end"])
    return times


def deepest(intervals):
    """How many of the intervals overlap at most at one instant."""
    edges = sorted([(start, 1) for start, _ in intervals] + [(end, -1) for _, end in intervals])
    depth = peak = 0
    for _, step in edges:
        depth += step
        peak = max(peak, depth)
    return peak


def check(attempts, out, concurrency):
    numbers = sorted(attempt["attempt"] for attempt in attempts)
    if numbers != list(range(1, ATTEMPTS + 1)):
        raise AssertionError(f"concurrency {concurrency}: attempts numbered {numbers}")
    if any(attempt["class"] != "pass" for attempt in attempts):
        raise AssertionError(f"concurrency {concurrency}: {[attempt['class'] for attempt in attempts]}")
    conversations = {attempt["conversation_id"] for attempt in attempts}
    copies = {path.name for path in (out / "work").iterdir() if path.is_dir()}
    if len(conversations) != ATTEMPTS or len(copies) != ATTEMPTS:
        raise AssertionError(f"concurrency {concurrency}: attempts shared a conversation or a workspace copy")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-e2e-concurrency-")
    root = Path(temporary.name).resolve()
    directory, failure, task_directory = root / "installation", None, None
    try:
        directory.mkdir(mode=0o700)
        (root / "scenario").mkdir(mode=0o755)
        scenario = root / "scenario" / "current.json"
        task = sorted(path.name for path in TASKS.iterdir() if path.is_dir())[0]
        # Apart from the private installation directory, so the check's own user can read the copy.
        task_directory = tempfile.TemporaryDirectory(prefix="tqa-e2e-concurrency-tasks-")
        tasks = Path(task_directory.name).resolve()
        slow_task(tasks, task)
        scenario.write_text((TASKS / task / "dry-run" / "solve.json").read_text())
        scenario.chmod(0o644)
        env["TQA_E2E_SCENARIO"] = str(scenario)
        images = {"runtime": image_id(env, env.get("TQA_RUNTIME_IMAGE", "tqa-runtime:base"))}
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
            compose.prepare(directory, env, runtime=images["runtime"], port=port)
        images["gateway"] = image_id(env, env["TQA_GATEWAY_IMAGE"])
        images["workstation"] = image_id(env, env.get("TQA_WORKSTATION_IMAGE", "tqa-runtime:workstation"))
        compose.ownership(directory, env)
        models = directory / "models.toml"
        compose.write(models, models.read_text() + "context_window=131072\nstream_usage=true\n")
        configure_templates(directory / "gateway.toml", images)
        compose.ownership(directory, env, "1000:1000")
        compose.command(COMPOSE + ["up", "-d", "--no-build", "--wait", "--wait-timeout", "90"], env)
        origin = "http://127.0.0.1:{}".format(port)
        token = root / "token"
        api_token(env, origin, token)
        one, one_out = run_at(env, root, origin, images, token, tasks, task, 1, [])
        samples = []
        four, four_out = run_at(env, root, origin, images, token, tasks, task, 4, samples)
        check(one, one_out, 1)
        check(four, four_out, 4)
        if comparable(one) != comparable(four):
            raise AssertionError(f"the runs differ: {comparable(one)} vs {comparable(four)}")
        peak = max(samples, default=0)
        if peak != CONTAINERS:
            raise AssertionError(f"at the peak {peak} harness containers ran at once (exactly {CONTAINERS} expected)")
        times = check_times(four_out)
        overlap = deepest([(start, end) for _, start, end in times.values()])
        if overlap > CONTAINERS:
            raise AssertionError(f"{overlap} checks overlapped (at most {CONTAINERS})")
        waits = {name: start - ready for name, (ready, start, _) in times.items()}
        if max(waits.values()) < WAITED_SECS:
            raise AssertionError(f"no check waited for a slot: {waits}")
        print(f"TK-I-006: {task} x {ATTEMPTS} at concurrency 1 and 4: the same attempts, each its own workspace, "
              f"exactly {peak} harness containers at the peak ({len(samples)} samples), checks at most {overlap} "
              f"deep, the longest wait for a slot {max(waits.values()):.1f} s", flush=True)
    except BaseException as error:
        failure = error
        if not isinstance(error, subprocess.SubprocessError):
            print("TK-I-006 failed: " + compose.redact(str(error), env), file=sys.stderr)
    finally:
        failure = compose.retire(env, failure, configuration=COMPOSE)
        compose.cleanup(directory, env, temporary, failure, compose=COMPOSE)
        if task_directory is not None:
            task_directory.cleanup()


if __name__ == "__main__":
    try:
        main()
    except (OSError, subprocess.SubprocessError):
        raise SystemExit("TK-I-006 failed; see diagnostics above") from None
