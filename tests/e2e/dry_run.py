"""TK-I-001: every E2E task dry-run against the scripted model on an owned stack.

Each seed must fail its own check; each task's scripted solution must pass and its scripted wrong answer must be
classified model.wrong_work. Needs TQA_DOCKER_TESTS=1, the built images, and host access to the stack's storage,
because the driver seeds workspaces through a container. Run it on its own, not beside another stack job.
"""
import argparse
import http.cookiejar
import json
import os
from pathlib import Path
import re
import secrets
import socket
import subprocess
import sys
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "deploy" / "tests"))
import compose  # noqa: E402  (the stack helpers live with the other stack suites)

COMPOSE = ["docker", "compose", "-f", "deploy/compose/docker.yml", "-f", "tests/e2e/compose/e2e.yml"]
TASKS = ROOT / "tests" / "e2e" / "tasks"
EXPECTED = {"solve": "pass", "wrong": "model.wrong_work"}
RESPONSE_BYTES = 1048576


def image_id(env, reference):
    return compose.command(["docker", "image", "inspect", "--format", "{{.Id}}", reference], env, capture=True).stdout.strip()


def request(client, origin, path, body):
    headers = {"Origin": origin, "Content-Type": "application/json", "Idempotency-Key": secrets.token_hex(16)}
    req = urllib.request.Request(origin + path, headers=headers, data=json.dumps(body).encode())
    with client.open(req, timeout=15) as response:
        data = response.read(RESPONSE_BYTES + 1)
        if len(data) > RESPONSE_BYTES:
            raise AssertionError("an HTTP response exceeded its bound")
        return json.loads(data)


def api_token(env, origin, path):
    """Bootstraps the administrator, signs in and stores a new API token in a private file; it is never printed."""
    client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    password = secrets.token_urlsafe(24)
    account = {"email": "e2e@example.invalid", "password": password}
    request(client, origin, "/api/v1/auth/bootstrap", dict(account, token=env["TQA_BOOTSTRAP_TOKEN"]))
    request(client, origin, "/api/v1/auth/login", account)
    # The creation response carries the secret, once, under "token" beside the flattened metadata.
    secret = request(client, origin, "/api/v1/tokens", {"name": "e2e-dry-run", "expires_in_days": 1})["token"]
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w") as file:
        file.write(secret + "\n")


def driver(env, *arguments):
    command = ["cargo", "run", "-q", "-p", "tqa-testkit", "--bin", "tqa-e2e", "--"] + [str(argument) for argument in arguments]
    return subprocess.run(command, cwd=ROOT, env=env, timeout=1800, check=False, stdout=subprocess.PIPE, text=True)


def serve(env, scenario, source, configuration=COMPOSE, services=("fake-model",)):
    """Rewrites the served scenario in place, keeping the bind-mounted inode, and restarts the fake models."""
    with open(scenario, "w") as file:
        file.write(source.read_text())
    compose.command(configuration + ["restart", *services], env)
    compose.command(configuration + ["up", "-d", "--no-build", "--no-deps", "--wait", "--wait-timeout", "60", *services], env)


def compaction_events(out):
    """Every compaction in the run's event logs: the transcript record the compaction demo asks for."""
    found = []
    for log in sorted((out / "events").glob("*.jsonl")):
        for line in log.read_text().splitlines():
            frame = json.loads(line)
            event = frame.get("event") or {}
            if event.get("type") == "compaction":
                found.append({"seq": frame.get("seq"), "source": event.get("source"), "through": event.get("through_seq")})
    return found


def verify(env, root, origin, images, tasks, expect_compaction):
    token = root / "token"
    api_token(env, origin, token)
    seeds = driver(env, "--seed-check", "--tasks", tasks, "--runtime-image", images["runtime"],
                   "--workstation-image", images["workstation"])
    print(seeds.stdout, end="", flush=True)
    if seeds.returncode != 0:
        raise AssertionError("a seed does not fail its own check")
    failures = []
    for task in sorted(path.name for path in tasks.iterdir() if path.is_dir()):
        for variant, expected in EXPECTED.items():
            serve(env, env["TQA_E2E_SCENARIO"], tasks / task / "dry-run" / f"{variant}.json")
            out = root / "runs" / f"{task}-{variant}"
            run = driver(env, "--dry-run", "--base-url", origin, "--token-file", token, "--route", "local",
                         "--tasks", tasks, "--only", task, "--out", out, "--storage-root", env["TQA_STORAGE_ROOT"],
                         "--runtime-image", images["runtime"], "--gateway-image", images["gateway"],
                         "--workstation-image", images["workstation"], "--repository", ROOT)
            lines = (out / "attempts.jsonl").read_text().splitlines() if run.returncode == 0 else []
            attempts = [json.loads(line) for line in lines]
            classes = [attempt["class"] for attempt in attempts]
            details = [attempt["detail"] for attempt in attempts if attempt.get("detail")]
            verdict = "ok " if classes == [expected] else "BAD"
            print(f"{verdict} {task} {variant}: {classes or 'driver exit ' + str(run.returncode)} (expected {expected})"
                  + (f"; {details}" if details else ""), flush=True)
            if classes != [expected]:
                failures.append(f"{task} {variant}")
            if expect_compaction and variant == "solve" and run.returncode == 0:
                events = compaction_events(out)
                for event in events:
                    print(f"    compaction at seq {event['seq']}: source {event['source']}, replaces through seq {event['through']}", flush=True)
                if not events:
                    failures.append(f"{task} {variant}: no compaction")
    if failures:
        raise AssertionError("unexpected dry-run classes: " + ", ".join(failures))


def configure_templates(config, images):
    """Offers Minimal (the default, as for tasks without a template) and Workstation, by immutable image ID."""
    content = config.read_text()
    for name, label, capabilities, default in (
            ("minimal", "Minimal", ["shell"], True),
            ("workstation", "Workstation", ["shell", "python", "node", "build", "browser"], False)):
        image = images["runtime" if name == "minimal" else name]
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", image):
            raise ValueError("Template images must resolve to immutable local IDs")
        content += "\n[[templates]]\nname={}\ndisplay_name={}\nimage={}\ncapabilities={}\ndefault={}\n".format(
            json.dumps(name), json.dumps(label), json.dumps(image), json.dumps(capabilities), "true" if default else "false")
    compose.write(config, content)


def save_logs(env, target, configuration=COMPOSE, services=("gateway", "fake-model")):
    """Redacted service and sandbox logs, for diagnosing one run; a failure to save never fails the run."""
    try:
        target.mkdir(parents=True, exist_ok=True)
        for service in services:
            logs = compose.command(configuration + ["logs", "--no-color", service], env, capture=True).stdout
            (target / (service + ".log")).write_text(compose.redact(logs, env))
        sandboxes = compose.command(["docker", "ps", "-aq", "--filter", "label=tqa.instance=" + env["TQA_SANDBOX_NETWORK"]],
                                    env, capture=True).stdout.split()
        for sandbox in sandboxes:
            logs = subprocess.run(["docker", "logs", sandbox], env=env, capture_output=True, text=True, timeout=60)
            (target / ("sandbox-" + sandbox[:12] + ".log")).write_text(compose.redact(logs.stdout + logs.stderr, env))
        print("Logs saved to " + str(target), flush=True)
    except (OSError, subprocess.SubprocessError) as error:
        print("Logs not saved: " + compose.redact(str(error), env), file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tasks", type=Path, default=TASKS, help="the task directory (default: the E2E task set)")
    parser.add_argument("--context-window", type=int, default=131072, help="the fake route's context window in tokens")
    parser.add_argument("--logs", type=Path, help="save the redacted gateway, fake-model and sandbox logs here before teardown")
    parser.add_argument("--expect-compaction", action="store_true",
                        help="require and print the compaction events of each solving run (the compaction demo)")
    args = parser.parse_args()
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-e2e-")
    root = Path(temporary.name).resolve()
    directory, failure = root / "installation", None
    try:
        directory.mkdir(mode=0o700)
        (root / "scenario").mkdir(mode=0o755)
        scenario = root / "scenario" / "current.json"
        scenario.write_text("{}")
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
        compose.write(models, models.read_text() + f"context_window={args.context_window}\nstream_usage=true\n")
        configure_templates(directory / "gateway.toml", images)
        compose.ownership(directory, env, "1000:1000")
        tasks = args.tasks.resolve()
        serve_first = tasks / sorted(path.name for path in tasks.iterdir() if path.is_dir())[0] / "dry-run" / "solve.json"
        scenario.write_text(serve_first.read_text())
        compose.command(COMPOSE + ["up", "-d", "--no-build", "--wait", "--wait-timeout", "90"], env)
        verify(env, root, "http://127.0.0.1:{}".format(port), images, tasks, args.expect_compaction)
        print("TK-I-001: every seed fails, every solution passes and every wrong answer is model.wrong_work", flush=True)
    except BaseException as error:
        failure = error
        if not isinstance(error, subprocess.SubprocessError):
            print("TK-I-001 failed: " + compose.redact(str(error), env), file=sys.stderr)
    finally:
        if args.logs:
            save_logs(env, args.logs)
        failure = compose.retire(env, failure, configuration=COMPOSE)
        compose.cleanup(directory, env, temporary, failure, compose=COMPOSE)


if __name__ == "__main__":
    try:
        main()
    except (OSError, subprocess.SubprocessError):
        raise SystemExit("TK-I-001 failed; see diagnostics above") from None
