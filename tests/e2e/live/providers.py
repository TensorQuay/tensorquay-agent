"""E2E-PROVIDER-014, the live provider suite, and its dry mode (TK-U-018), on an owned stack.

Dry mode (the default): the suite's four routes, with the settings we ship, point at one scripted fake per provider
profile (tests/e2e/live/compose/dry.yml). Per route: the route probe must reach the model without a refusal, then the
smoke task's scripted solution must pass and its scripted wrong answer must be classified model.wrong_work. No key and
no paid endpoint is involved.

Live mode (--live, SPENDS REAL QUOTA; only with the lead's OK for that run): the same routes against the real endpoints,
with each route's model, base URL and key variable from the owner's private routes file and its key only in the
gateway's seed. Per route: the route probe must be verified, then the smoke task runs once under a 30k-token cap.
Without --yes it prints the plan and stops. Results go to --report-dir (outside the repository); the output shows
neither keys nor base URLs nor model names.

Needs TQA_DOCKER_TESTS=1, the built images, and host access to the stack's storage. Run it on its own: it starts a
whole stack.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import tempfile
import urllib.request

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(ROOT / "deploy" / "tests"))
sys.path.insert(0, str(ROOT / "tests" / "e2e"))
import compose  # noqa: E402
import dry_run  # noqa: E402  (the owned-stack helpers of TK-I-001)
import live  # noqa: E402
import seed  # noqa: E402

LIVE_COMPOSE = ["docker", "compose", "-f", "deploy/compose/docker.yml"]
COMPOSE = LIVE_COMPOSE + ["-f", "tests/e2e/live/compose/dry.yml"]
# The live run's cap per route: the smoke task's budget; the probe adds up to four short requests.
LIVE_TASK_TOKENS = 30000
TASKS = HERE / "tasks"
EXPECTED = {"solve": "pass", "wrong": "model.wrong_work"}
# A probe that fails with one of these never got past the endpoint's rules or the route's wiring.
REFUSED = {"upstream_error", "auth_rejected", "not_found", "rate_limited", "protocol_error"}

# The suite's routes: neutral names, the protocol, the fake profile standing in for the provider, and the settings the
# live run uses for that provider.
ROUTES = [
    {"name": "p1-chat", "protocol": "openai_chat", "fake": "fake-chat-strict", "settings": ""},
    {"name": "p2-chat-basic", "protocol": "openai_chat", "fake": "fake-chat-compat",
     "settings": "schema_keywords='basic'\nreasoning={field='reasoning_effort',allowed=['low'],"
                 "default='low'}\n"},
    {"name": "p3-responses", "protocol": "openai_responses", "fake": "fake-responses-backend", "settings": ""},
    {"name": "p4-messages", "protocol": "anthropic_messages", "fake": "fake-messages",
     "settings": "max_output=16000\nreasoning={field='thinking',allowed=['medium'],default='medium',"
                 "budget_tokens={medium=4096}}\n"},
]
FAKES = tuple(route["fake"] for route in ROUTES)


def models_toml(context_window):
    """The four routes, keyed on the installation's synthetic secret."""
    return "".join(
        "[[routes]]\nname='{name}'\nprotocol='{protocol}'\nbase_url='http://{fake}:8080/v1'\nmodel='fixture'\n"
        "auth={{kind='bearer'}}\nkey_ref='secrets/local'\nnative_tools=true\nstream_usage=true\n"
        "context_window={window}\n{settings}".format(window=context_window, **route)
        for route in ROUTES)


def probe(origin, token, name):
    """The route probe through the admin API with the run's API token; returns the stored result."""
    headers = {"Origin": origin, "Authorization": "Bearer " + token, "Idempotency-Key": secrets.token_hex(16)}
    req = urllib.request.Request(origin + "/api/v1/admin/routes/{}/probe".format(name), headers=headers, data=b"",
                                 method="POST")
    with urllib.request.urlopen(req, timeout=90) as response:
        return json.loads(response.read(dry_run.RESPONSE_BYTES))["probe"]


def verify(env, root, origin, images):
    token_file = root / "token"
    dry_run.api_token(env, origin, token_file)
    token = token_file.read_text().strip()
    failures = []
    for route in ROUTES:
        result = probe(origin, token, route["name"])
        failure = (result.get("observed") or {}).get("failure") or {}
        reached = result.get("state") == "verified" or failure.get("code") not in REFUSED
        print("{} {} probe: {}{}".format("ok " if reached else "BAD", route["name"], result.get("state"),
                                         " " + json.dumps(failure) if failure else ""), flush=True)
        if not reached:
            failures.append(route["name"] + " probe")
        for variant, expected in EXPECTED.items():
            dry_run.serve(env, env["TQA_E2E_SCENARIO"], TASKS / "smoke-fix" / "dry-run" / (variant + ".json"),
                          COMPOSE, FAKES)
            out = root / "runs" / "{}-{}".format(route["name"], variant)
            run = dry_run.driver(env, "--dry-run", "--base-url", origin, "--token-file", token_file,
                                 "--route", route["name"], "--tasks", TASKS, "--out", out,
                                 "--storage-root", env["TQA_STORAGE_ROOT"], "--runtime-image", images["runtime"],
                                 "--gateway-image", images["gateway"], "--workstation-image", images["workstation"],
                                 "--repository", ROOT)
            lines = (out / "attempts.jsonl").read_text().splitlines() if run.returncode == 0 else []
            attempts = [json.loads(line) for line in lines]
            classes = [attempt["class"] for attempt in attempts]
            details = [attempt["detail"] for attempt in attempts if attempt.get("detail")]
            print("{} {} {}: {} (expected {}){}".format(
                "ok " if classes == [expected] else "BAD", route["name"], variant,
                classes or "driver exit " + str(run.returncode), expected, "; " + str(details) if details else ""),
                flush=True)
            if classes != [expected]:
                failures.append("{} {}".format(route["name"], variant))
    if failures:
        raise AssertionError("unexpected dry-mode results: " + ", ".join(failures))


def verify_live(env, root, origin, images, routes, guard, probe_only=False):
    """Per route: the probe, then (only when verified) one smoke-task attempt under the cap. Returns the result rows."""
    token_file = root / "token"
    dry_run.api_token(env, origin, token_file)
    token = token_file.read_text().strip()
    rows = []
    for route in routes:
        result = probe(origin, token, route["name"])
        observed = result.get("observed") or {}
        failure = observed.get("failure") or {}
        state = result.get("state") or "unknown"
        if failure:
            hint = failure.get("suggestion")
            state += " ({}{})".format(failure.get("code"), ", try {}={}".format(hint["field"], hint["value"]) if hint else "")
        tokens = observed.get("tokens") or {}
        # What the endpoint answered, for the private report: HTTP status and the gateway-redacted excerpt of a
        # refusal, whether the answer streamed, and how soon it came.
        detail = {key: value for key, value in (("status", failure.get("status")),
                                                ("excerpt", failure.get("excerpt")),
                                                ("detail", failure.get("detail")),
                                                ("streaming", observed.get("streaming")),
                                                ("first_byte_ms", observed.get("first_byte_ms")),
                                                ("tool_call_extras", observed.get("tool_call_extras")),
                                                # Which step of the probe failed: its checks in order.
                                                ("checks", {key: observed.get(key) for key in
                                                            ("tool_call", "round_trip", "strict_line_breaks")
                                                            if observed.get(key) is not None} or None))
                  if value is not None}
        row = {"route": route["name"], "protocol": route["protocol"], "settings": live.settings_label(route),
               "model": route["model"], "probe": state, "task": "not run", "prompt": tokens.get("prompt", 0),
               "completion": tokens.get("completion", 0), "requests": "n/a", "detail": json.dumps(detail)}
        print(guard.check("{} probe: {} {}".format(route["name"], state, row["detail"])), flush=True)
        if result.get("state") == "verified" and not probe_only:
            out = root / "runs" / route["name"]
            run = dry_run.driver(env, "--base-url", origin, "--token-file", token_file, "--route", route["name"],
                                 "--tasks", TASKS, "--out", out, "--storage-root", env["TQA_STORAGE_ROOT"],
                                 "--runtime-image", images["runtime"], "--gateway-image", images["gateway"],
                                 "--workstation-image", images["workstation"], "--repository", ROOT,
                                 "--allow-paid-route", "--max-total-tokens", LIVE_TASK_TOKENS, "--yes")
            lines = (out / "attempts.jsonl").read_text().splitlines() if (out / "attempts.jsonl").is_file() else []
            attempts = [json.loads(line) for line in lines]
            row["task"] = ", ".join(attempt["class"] for attempt in attempts) or "driver exit " + str(run.returncode)
            for detail in live.attempt_details(attempts):
                print(guard.check("{} smoke-fix detail: {}".format(route["name"], detail)), flush=True)
                shown = json.loads(row["detail"])
                shown.setdefault("task", []).append(detail)
                row["detail"] = json.dumps(shown)
            prompt, completion, steps = live.attempt_totals(attempts)
            row["prompt"] += prompt
            row["completion"] += completion
            row["requests"] = steps
            print(guard.check("{} smoke-fix: {}".format(route["name"], row["task"])), flush=True)
        rows.append(row)
    return rows


def report(rows, report_dir, guard, commit):
    """The private report (with model names) under report_dir, and the neutral summary for the repository."""
    import datetime
    now = datetime.datetime.now().astimezone()
    private = ["# Live provider suite (E2E-PROVIDER-014), {} at {}".format(now.strftime("%Y-%m-%d %H:%M %z"), commit), "",
               "| Route | Model | Probe detail |", "|---|---|---|"]
    private += ["| {} | {} | `{}` |".format(row["route"], row["model"], row["detail"].replace("|", "/")) for row in rows]
    neutral = live.summary(rows)
    text = "\n".join(private) + "\n\n" + neutral
    guard.check(neutral)
    for secret in guard.secrets:
        if secret in text:
            raise live.LiveError("a key value or a base URL reached the report; nothing was written")
    report_dir.mkdir(parents=True, exist_ok=True)
    path = report_dir / "providers-{}.md".format(now.strftime("%Y-%m-%d-%H%M"))
    path.write_text(text)
    print(neutral, end="", flush=True)
    print("Report: " + str(path), flush=True)


def plan(routes):
    print("E2E-PROVIDER-014 live plan (spends real quota):")
    for route in routes:
        print("  {} ({}, {}): probe, then smoke-fix once if verified; cap {} tokens".format(
            route["name"], route["protocol"], live.settings_label(route), LIVE_TASK_TOKENS))
    print("Worst case about {} tokens in all. Add --yes to run.".format(len(routes) * (LIVE_TASK_TOKENS + 2000)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--context-window", type=int, default=131072, help="each route's context window in tokens")
    parser.add_argument("--logs", type=Path, help="dry mode: save the redacted service and sandbox logs here")
    parser.add_argument("--live", action="store_true", help="run against the real endpoints (spends quota)")
    parser.add_argument("--routes-file", type=Path, help="live: the owner's private routes file (mode 0600)")
    parser.add_argument("--env-file", type=Path, help="live: the owner's private key file (mode 0600)")
    parser.add_argument("--report-dir", type=Path, default=Path.home() / "team" / "evals",
                        help="live: where the report goes (outside the repository)")
    parser.add_argument("--yes", action="store_true", help="live: run after the plan instead of stopping")
    parser.add_argument("--only", action="append", default=[], help="live: run only this route (repeatable)")
    parser.add_argument("--probe-only", action="store_true", help="live: probe the routes and run no task")
    args = parser.parse_args()
    if args.live:
        try:
            return main_live(args)
        except SystemExit:
            raise
        except BaseException:
            # No traceback: an exception's text is shown only through the guard, above.
            raise SystemExit("E2E-PROVIDER-014 failed; see the line above") from None
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-live-dry-")
    root = Path(temporary.name).resolve()
    directory, failure = root / "installation", None
    try:
        directory.mkdir(mode=0o700)
        (root / "scenario").mkdir(mode=0o755)
        scenario = root / "scenario" / "current.json"
        scenario.write_text((TASKS / "smoke-fix" / "dry-run" / "solve.json").read_text())
        scenario.chmod(0o644)
        env["TQA_E2E_SCENARIO"] = str(scenario)
        images = {"runtime": dry_run.image_id(env, env.get("TQA_RUNTIME_IMAGE", "tqa-runtime:base"))}
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
            print(live.stamp("prepare", None), flush=True)
            compose.prepare(directory, env, runtime=images["runtime"], port=port)
        images["gateway"] = dry_run.image_id(env, env["TQA_GATEWAY_IMAGE"])
        images["workstation"] = dry_run.image_id(env, env.get("TQA_WORKSTATION_IMAGE", "tqa-runtime:workstation"))
        compose.ownership(directory, env)
        compose.write(directory / "models.toml", models_toml(args.context_window))
        dry_run.configure_templates(directory / "gateway.toml", images)
        compose.ownership(directory, env, "1000:1000")
        print(live.stamp("up", env.get("COMPOSE_PROJECT_NAME")), flush=True)
        compose.command(COMPOSE + ["up", "-d", "--no-build", "--wait", "--wait-timeout", "90"], env)
        # As in the live mode: the seed leaves the disk once the gateway has read it (here synthetic).
        compose.command(live.seed_removal(str(directory), compose.TOYBOX), env, timeout=300)
        compose.command(live.seed_absent(str(directory), compose.TOYBOX), env, timeout=300)
        print(live.stamp("seed removed", env.get("COMPOSE_PROJECT_NAME")), flush=True)
        print(live.stamp("first test", env.get("COMPOSE_PROJECT_NAME")), flush=True)
        verify(env, root, "http://127.0.0.1:{}".format(port), images)
        print("TK-U-018: every route of the live suite reaches its provider fake and runs the smoke task", flush=True)
    except BaseException as error:
        failure = error
        if not isinstance(error, subprocess.SubprocessError):
            print("TK-U-018 failed: " + compose.redact(str(error), env), file=sys.stderr)
    finally:
        if args.logs:
            dry_run.save_logs(env, args.logs, COMPOSE, ("gateway",) + FAKES)
        print(live.stamp("down", env.get("COMPOSE_PROJECT_NAME")), flush=True)
        failure = compose.retire(env, failure, configuration=COMPOSE)
        try:
            compose.cleanup(directory, env, temporary, failure, compose=COMPOSE)
        finally:
            print(live.stamp("down done", env.get("COMPOSE_PROJECT_NAME")), flush=True)


def main_live(args):
    if args.logs:
        raise SystemExit("--logs is for the dry mode only")
    if not args.routes_file or not args.env_file:
        raise SystemExit("--live needs --routes-file and --env-file")
    try:
        values = seed.parse(seed.private_file(args.env_file))
        routes = live.resolve(live.private_routes(args.routes_file), ROUTES, values)
    except (seed.SeedError, live.LiveError) as error:
        raise SystemExit("live: " + str(error)) from None
    guard = live.Guard(routes, values)
    if set(args.only) - {route["name"] for route in routes}:
        raise SystemExit("--only names a route the suite does not have")
    if args.only:
        routes = [route for route in routes if route["name"] in args.only]
    plan(routes)
    if args.probe_only:
        print("--probe-only: no task runs; each probe is two short requests.")
    if not args.yes:
        return
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    commit = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=ROOT, capture_output=True, text=True,
                            check=False).stdout.strip()
    temporary = tempfile.TemporaryDirectory(prefix="tqa-live-")
    root = Path(temporary.name).resolve()
    directory, failure, seed_removed = root / "installation", None, False
    try:
        directory.mkdir(mode=0o700)
        images = {"runtime": dry_run.image_id(env, env.get("TQA_RUNTIME_IMAGE", "tqa-runtime:base"))}
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
            print(live.stamp("prepare", None), flush=True)
            compose.prepare(directory, env, runtime=images["runtime"], port=port)
        images["gateway"] = dry_run.image_id(env, env["TQA_GATEWAY_IMAGE"])
        images["workstation"] = dry_run.image_id(env, env.get("TQA_WORKSTATION_IMAGE", "tqa-runtime:workstation"))
        compose.ownership(directory, env)
        compose.write(directory / "models.toml", live.models_toml(routes, args.context_window))
        compose.write(directory / "secrets.toml",
                      seed.seed_text([(route["name"], route["key_env"]) for route in routes], values))
        dry_run.configure_templates(directory / "gateway.toml", images)
        compose.ownership(directory, env, "1000:1000")
        print(live.stamp("up", env.get("COMPOSE_PROJECT_NAME")), flush=True)
        compose.command(LIVE_COMPOSE + ["up", "-d", "--no-build", "--wait", "--wait-timeout", "90"], env)
        compose.command(live.seed_removal(str(directory), compose.TOYBOX), env, timeout=300)
        compose.command(live.seed_absent(str(directory), compose.TOYBOX), env, timeout=300)
        seed_removed = True
        print(live.stamp("seed removed", env.get("COMPOSE_PROJECT_NAME")), flush=True)
        print(live.stamp("first test", env.get("COMPOSE_PROJECT_NAME")), flush=True)
        rows = verify_live(env, root, "http://127.0.0.1:{}".format(port), images, routes, guard, args.probe_only)
        report(rows, args.report_dir, guard, commit)
    except BaseException as error:
        failure = error
        # Never show an error text that could carry a value: only its type, or the guarded text.
        try:
            shown = guard.check(compose.redact(str(error), env))
        except live.LiveError:
            shown = type(error).__name__
        print("E2E-PROVIDER-014 failed: " + shown, file=sys.stderr)
    finally:
        # The keys first: if the run ended before the seed was removed, remove it before anything else.
        if not seed_removed and directory.exists():
            try:
                compose.command(live.seed_removal(str(directory), compose.TOYBOX), env, timeout=300)
                compose.command(live.seed_absent(str(directory), compose.TOYBOX), env, timeout=300)
                seed_removed = True
            except (OSError, subprocess.SubprocessError):
                print("E2E-PROVIDER-014: THE SEED WITH THE KEYS IS STILL ON DISK in " + str(directory)
                      + "; remove it now", file=sys.stderr, flush=True)
        print(live.stamp("down", env.get("COMPOSE_PROJECT_NAME")), flush=True)
        failure = compose.retire(env, failure, configuration=LIVE_COMPOSE)
        try:
            compose.cleanup(directory, env, temporary, failure, compose=LIVE_COMPOSE)
        finally:
            print(live.stamp("down done", env.get("COMPOSE_PROJECT_NAME")), flush=True)
            if root.exists():
                print("E2E-PROVIDER-014: CLEANUP LEFT {} ON DISK (seed removed: {})".format(
                    root, "yes" if seed_removed else "NO"), file=sys.stderr, flush=True)


if __name__ == "__main__":
    try:
        main()
    except (OSError, subprocess.SubprocessError):
        raise SystemExit("E2E-PROVIDER-014 failed; see diagnostics above") from None
