"""WEB-E2E-004: an owned full-stack demo with a real sandbox runtime."""

import argparse
import json
import os
from pathlib import Path
import re
import secrets
import signal
import socket
import subprocess
import sys
import tempfile
import uuid

import compose
import demo_agents
import demo_decision_engines
import demo_desktop
import demo_files
import demo_key_use
import demo_previews
import demo_egress
import demo_environments
import demo_queue
import demo_task_page
import demo_run_ids
import demo_titles
import demo_ux
import demo_workspaces

COMPOSE = ["docker", "compose", "-f", "deploy/compose/docker.yml", "-f", "deploy/compose/demo.yml"]


def runtime_image(env, reference=None):
    identity = compose.command(["docker", "image", "inspect", "--format", "{{.Id}}", reference or compose.required_image(env, "TQA_RUNTIME_IMAGE")], env, capture=True).stdout.strip()
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", identity):
        raise ValueError("The built runtime must resolve to an immutable local image ID")
    return identity


# hello/README.md after the demo's two requests: created by the first, then changed by the second (DEV-PLAN Phase 1:
# "create a small project and then change it"; crates/tqa-testkit/scenarios/demo.json).
DEMO_README = "# Hello\nA small demonstration project.\n\n## Usage\nRead this file.\n"


def verify_workspace(env):
    result = json.loads(Path(env["TQA_STACK_RESULT"]).read_text())
    identity = str(uuid.UUID(result["conversationId"]))
    workspace = Path(env["TQA_STORAGE_ROOT"]) / "workspaces" / identity
    target = workspace / "hello" / "README.md"
    if not target.is_file():
        raise AssertionError("The runtime did not create hello/README.md in the conversation workspace")
    if workspace.resolve() not in target.resolve().parents:
        raise AssertionError("The created file escaped its workspace")
    if target.stat().st_size > 4096 or target.read_text() != DEMO_README:
        raise AssertionError("Unexpected workspace contents: hello/README.md isn't the changed project")
    print("Host workspace: hello/README.md exists, created and then changed, with the expected contents", flush=True)


def verify_catch_up(env, configuration):
    """WEB-E2E-121: the proxy must have held at least one event stream, or the catch-up was never exercised and the
    spec's pass says nothing. The proxy says so in its log (fixtures/buffering_proxy.py, held_line)."""
    log = compose.command(configuration + ["logs", "--no-color", "proxy"], env, capture=True).stdout
    held = [line for line in log.splitlines() if "buffering proxy: held " in line]
    if not held:
        raise AssertionError("The proxy held no event stream: the catch-up suite did not exercise the buffering")
    print("Buffering proxy: held {} event stream{}; the page caught up meanwhile".format(
        len(held), "" if len(held) == 1 else "s"), flush=True)


def verify_accounts(env):
    """Task 067: the member's token worked with curl before revocation and was refused after it."""
    text = Path(env["TQA_STACK_RESULT"]).read_text()
    if "tqa_pat_" in text:
        raise AssertionError("The result must never record a token")
    result = json.loads(text)
    if result != {"bearer": [200, 401], "joined": True}:
        raise AssertionError("Unexpected accounts result: {}".format(result))
    print("Accounts: invited, joined; Bearer token 200 before revoking and 401 after", flush=True)


def verify_channel_members(env):
    """WEB-U-066: after the page's changes the channel held its lead and the task's agent, then ended, then was gone."""
    result = json.loads(Path(env["TQA_STACK_RESULT"]).read_text())
    expected = {"members": ["conversation:member", "user:lead"], "state": "ended", "deleted": 404}
    if result != expected:
        raise AssertionError("Unexpected channel-members result: {}".format(result))
    print("Channel members: agent and person added, role changed, person removed; ended, then deleted", flush=True)


def verify_reply_cut(env):
    """WEB-E2E-028: each cut reply failed as reply_cut with the reason's exact numbers, and a new message then
    completed; a spent output budget failed as budget; a too-long input failed as the provider's error; and the page
    showed no raw code in any of them."""
    result = json.loads(Path(env["TQA_STACK_RESULT"]).read_text())
    cut = {"code": "reply_cut",
           "reason": "the model stopped at its output limit (finish: length) after 512 tokens; 2048 requested",
           "again": "completed"}
    # A reasoning-only cut says so in its own words.
    thinking = dict(cut, reason="the model spent its whole output limit thinking (512 tokens) and wrote no answer; "
                                "2048 requested")
    expected = {"text": cut, "thinking": thinking, "call": dict(cut, toolRan=False),
                "budget": {"code": "budget", "reason": "budget.output_tokens"},
                "tooLong": {"code": "provider_error"}, "rawShown": False}
    if result != expected:
        raise AssertionError("Unexpected reply-cut result: {}".format(result))
    print("Reply cut: a cut text, a cut thought and a cut tool call (never run) each failed as reply_cut with plain "
          "words, and a new message then answered; a spent budget and a too-long input failed with their own words; "
          "no raw code on the page", flush=True)


def verify_models_people(env):
    """WEB-E2E-026: each Models change read back from the gateway as the page showed it, the pasted key never answered
    back; a person's task listed as "Private task" on their page, its title and content kept from the administrator."""
    result = json.loads(Path(env["TQA_STACK_RESULT"]).read_text())
    key = "secrets/edit-RUN-key"
    expected = {"pasteKey": {"keyRef": key, "stored": [key], "shown": False},
                "listModels": {"status": 200, "failure": None, "models": ["fixture"]}, "picked": "fixture",
                "edited": {"base_url": "http://127.0.0.1:9/v1", "protocol": "openai_responses", "concurrency": 2,
                           "max_output": 1024, "strict": "off", "schema_keywords": "basic",
                           "extra_headers": {"X-Stack-Check": "yes"}, "key_ref": key},
                "listAfterEdit": {"status": 200, "failure": "unreachable", "models": []},
                "deleted": True, "keyInResponses": False,
                "person": {"listed": True, "listedTitle": False, "readStatus": 200, "readTitle": False,
                           "titleShown": False, "followUpShown": False, "eventsStatus": 403}}
    if result != expected:
        raise AssertionError("Unexpected models-people result: {}".format(result))
    print("Models: a key pasted and stored, never answered back; models listed; address, protocol, limits, tool settings "
          "and headers edited and read back; the new address unreachable; deleted. People: a person's page lists their "
          "task as a private task, its title and content kept from the administrator",
          flush=True)


def verify_settings(env):
    """WEB-E2E-025: each change the Settings round trip made was read back from the gateway as the page showed it."""
    result = json.loads(Path(env["TQA_STACK_RESULT"]).read_text())
    expected = {"account": "theme and language applied and remembered", "limit": {"set": True, "reset": True},
                "rulePorts": [443, 8443], "sharedKey": True,
                "profiles": [["copy", False, "Checks every setting once."], ["shared", True, "Checks every setting once."]],
                "workspaces": {"renamed": True, "deleted": True}, "signedOut": 401,
                # Every value below was read from the gateway, never set by the test itself.
                "people": {"roles": ["admin", "member"], "session": [200, 401, 200], "invitation": [True, False]},
                "models": {"description": "For a quick check.", "checked": "verified", "keyAllowed": True,
                           "downloaded": True, "deleted": True}}
    if result != expected:
        raise AssertionError("Unexpected settings result: {}".format(result))
    print("Settings: limit set and reset, rule edited, shared key added, profiles shared and copied, workspace renamed "
          "and deleted, signed out; a person invited, joined, promoted and back, disabled and enabled, an invitation "
          "revoked; a route added, described, checked, downloaded and deleted", flush=True)


# The browsers the stack suites run in (web/playwright.stack.config.ts); WebKit stands in for Safari.
BROWSERS = ("chromium", "webkit")

def verify_task_page(env, restarter):
    """The suite's own result; then the restarter stops (T13's spec asserts its restart itself, through the markers)."""
    demo_task_page.verify_result(env)
    if restarter is not None:
        restarter.finish()


def browser(env):
    args = ["corepack", "pnpm", "exec", "playwright", "test", "--config", "playwright.stack.config.ts"]
    print("Browser: " + env.get("TQA_STACK_BROWSER", "chromium"), flush=True)
    if env.get("TQA_STACK_GREP"):
        args += ["--grep", env["TQA_STACK_GREP"]]
    try:
        result = compose.command(args, env, capture=True, cwd="web",
                                 # Two agent-team runs, each with several task starts and a gateway restart; or seven
                                 # Desktop cases, each starting a Desktop sandbox, one with a restart.
                                 timeout=1800 if env.get("TQA_STACK_SUITE") in ("agents", "desktop") else
                                 600 if env.get("TQA_STACK_SUITE") in ("ux", "environment", "queue", "egress", "accounts", "files",
                                                                        "workspaces", "channel-members", "models-people", "settings",
                                                                        "task-page", "reply-cut", "key-use", "previews",
                                                                        "titles", "basic") else 90)
        print(compose.redact(result.stdout, env), end="", flush=True)
    except subprocess.SubprocessError as error:
        output = error.output or ""
        if isinstance(output, bytes):
            output = output.decode("utf-8", errors="replace")
        print("\n".join(compose.redact(output, env).splitlines()[-80:]), file=sys.stderr)
        raise


def keep(directory, env):
    compose.write(directory / "browser.env", "TQA_BOOTSTRAP_TOKEN=" + env["TQA_BOOTSTRAP_TOKEN"] + "\n")
    print("Bootstrap token file: {}".format(directory / "browser.env"), flush=True)
    print("Stack remains available until Ctrl-C; shutdown removes this temporary installation.", flush=True)
    signal.pause()


def report(error, env):
    """A failure found after the browser's output names itself; otherwise the log ends at "see diagnostics above"."""
    print("Demo contract failed: {}: {}".format(type(error).__name__, compose.redact(str(error), env)),
          file=sys.stderr, flush=True)


def verify(suite, check, env):
    """The suite's own check of what its browser run left behind, or the failure it found (and reported). A run
    narrowed by TQA_STACK_GREP left only part of that behind, so the check is skipped, and the log says so."""
    if env.get("TQA_STACK_GREP"):
        print("Only the tests matching TQA_STACK_GREP ran, so the {} suite's own check is skipped".format(suite),
              flush=True)
        return None
    try:
        check(env)
    except Exception as error:
        report(error, env)
        return error
    return None


def keep_logs(env, failure, configuration):
    """After a failure (not Ctrl-C), the stack's logs go to `TQA_WEB_EVIDENCE_DIR`/stack-logs, else to a new folder
    that outlives this temporary installation, so the evidence of a failure that does not repeat is never lost."""
    if failure is None or isinstance(failure, KeyboardInterrupt):
        return []
    evidence = env.get("TQA_WEB_EVIDENCE_DIR")
    folder = Path(evidence) / "stack-logs" if evidence else Path(tempfile.mkdtemp(prefix="tqa-demo-logs-"))
    written = compose.save_logs(env, folder, configuration=configuration)
    if not written and not evidence:
        folder.rmdir()
    return written


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep", action="store_true", help="Keep the stack available until Ctrl-C instead of running the browser contract")
    parser.add_argument("--suite", choices=("basic", "agents", "ux", "environment", "queue", "egress", "accounts", "files",
                                                 "workspaces", "channel-members", "models-people", "settings", "task-page",
                                                 "reply-cut", "key-use", "previews", "titles", "desktop", "catch-up"),
                        default="basic",
                        help="Select the full-stack contract")
    parser.add_argument("--backend", choices=("docker", "firecracker"), default="docker",
                        help="previews only: firecracker needs a KVM host, TQA_VMM_IMAGE and TQA_FC_KERNEL_DIR")
    args = parser.parse_args()
    if args.backend == "firecracker" and args.suite != "previews":
        raise SystemExit("--backend firecracker is for the previews suite")
    if os.environ.get("TQA_STACK_BROWSER", "chromium") not in BROWSERS:
        raise SystemExit("TQA_STACK_BROWSER must be one of " + ", ".join(BROWSERS))
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    host = "127.0.0.1" if args.suite == "basic" else demo_ux.lan_host(env)
    configuration = COMPOSE + (["-f", "deploy/compose/{}.yml".format(args.suite)] if args.suite in ("agents", "environment", "queue", "egress", "files", "workspaces", "task-page", "previews",
                                                                                                                "titles", "desktop", "catch-up")
                             else [])
    if args.backend == "firecracker":
        # First, so that tearing down after any failure can still interpolate the overlay.
        configuration = demo_previews.microvm_overlay(configuration, env)
    witness = demo_previews.Witness(host).start() if args.suite == "previews" else None
    restarter = pictures = None
    env.update(TQA_STACK_SUITE=args.suite, TQA_DEMO_HOST=host,
               TQA_DEMO_SCENARIO={"ux": "demo-long.json", "reply-cut": "reply-cut.json"}.get(args.suite, "demo.json"))
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-demo-")
    root = Path(temporary.name).resolve()
    directory, failure = root / "installation", None
    try:
        directory.mkdir(mode=0o700)
        with socket.socket() as reservation:
            reservation.bind((host, 0))
            port = reservation.getsockname()[1]
            minimal = runtime_image(env)
            # The Workstation under test, never the shared engine's bare tag (`compose.builtin_pins` refuses a run
            # that doesn't name it).
            images = {"minimal": minimal, "workstation": compose.builtin_pins(env, minimal, stand=False)["workstation"]}
            compose.prepare(directory, env, runtime=images["minimal"], port=port, host=host,
                            max_live=12 if args.suite == "ux" else 4)
            if args.backend == "firecracker":
                demo_previews.to_microvm(directory, env, images["minimal"])
            else:
                demo_environments.configure(directory, env, images)
            if args.suite == "previews":
                demo_previews.configure(directory, env, witness)
            if args.suite == "settings":
                demo_decision_engines.configure(directory, env)  # WEB-E2E-070's two engines
            if args.suite == "queue":
                demo_queue.configure(directory, env)
            elif args.suite == "agents":
                demo_agents.configure(directory, env)
            elif args.suite == "egress":
                demo_egress.configure(directory, env)
            elif args.suite == "workspaces":
                demo_workspaces.configure(directory, env)
            elif args.suite == "titles":
                demo_titles.configure(directory, env)
            elif args.suite == "desktop":
                demo_desktop.configure(directory, env)
        env.update(TQA_STACK_URL="http://{}:{}".format(host, port),
                   TQA_STACK_PASSWORD="synthetic-demo-" + secrets.token_hex(16),
                   TQA_STACK_RESULT=str(root / "browser.json"))
        if args.suite == "agents":
            demo_agents.verify_overlay(env, configuration)
        elif args.suite == "environment":
            demo_environments.verify_overlay(env, configuration)
        elif args.suite == "queue":
            demo_queue.verify_overlay(env, configuration)
        elif args.suite == "egress":
            demo_egress.verify_overlay(env, configuration)
        elif args.suite == "files":
            demo_files.verify_overlay(env, configuration)
        elif args.suite == "workspaces":
            demo_workspaces.verify_overlay(env, configuration)
        elif args.suite == "task-page":
            demo_task_page.verify_overlay(env, configuration)
        elif args.suite == "previews":
            demo_previews.verify_overlay(env, configuration)
        elif args.suite == "titles":
            demo_titles.verify_overlay(env, configuration)
        elif args.suite == "desktop":
            demo_desktop.verify_overlay(env, configuration)
        compose.command(configuration + ["up", "-d", "--no-build", "--wait", "--wait-timeout",
                                         "120" if args.backend == "firecracker" else "60"], env, timeout=180)
        print("Gateway URL: " + env["TQA_STACK_URL"], flush=True)
        if args.suite == "key-use":
            demo_key_use.break_audit(configuration, env)
        if args.suite == "agents":
            env["TQA_AGENTS_MARKERS"] = str(root / "markers")
            restarter = demo_agents.Restarter(env["TQA_AGENTS_MARKERS"], configuration, env).start()
        elif args.suite == "task-page":
            # The same restarter, for task-page-c15-stack.spec.ts's restart mid-step (the c15 feature matrix, T13).
            env["TQA_TASK_PAGE_MARKERS"] = str(root / "markers")
            restarter = demo_agents.Restarter(env["TQA_TASK_PAGE_MARKERS"], configuration, env).start()
        elif args.suite == "desktop":
            # The same restarter, for WEB-E2E-073's gateway restart during a Take over.
            env["TQA_DESKTOP_MARKERS"] = str(root / "markers")
            restarter = demo_agents.Restarter(env["TQA_DESKTOP_MARKERS"], configuration, env).start()
            # WEB-E2E-071's pictures, checked on disk while its task exists (the afterEach deletes every task).
            pictures = demo_desktop.PictureWatch(env["TQA_DESKTOP_MARKERS"], env).start()
        keep(root, env) if args.keep else browser(env)
        if args.suite == "basic" and not args.keep:
            demo_run_ids.record(configuration, env)
    except KeyboardInterrupt as error:
        print("Stopping the owned demo stack", flush=True)
        if not args.keep:
            failure = error
    except BaseException as error:
        failure = error
        if not isinstance(error, subprocess.SubprocessError):
            print("Demo contract failed: " + compose.redact(str(error), env), file=sys.stderr)
    finally:
        try:
            demo_key_use.mend_audit(configuration, env)
        except Exception as error:
            report(error, env)
            failure = failure or error
        kept = keep_logs(env, failure, configuration)
        if kept and args.suite == "agents":
            demo_agents.save_team_state(env, kept[0].parent, configuration)
        failure = compose.retire(env, failure, configuration=configuration)
        if failure is None and not args.keep:
            try:
                compose.ownership(directory, env)
            except Exception as error:
                failure = error
                report(error, env)
            else:
                checks = {"ux": demo_ux.verify_workspaces, "environment": demo_environments.verify_workspace,
                          "queue": demo_queue.verify_result,
                          "egress": demo_egress.verify_result, "accounts": verify_accounts,
                          "files": demo_files.verify_result,
                          "workspaces": demo_workspaces.verify_result, "channel-members": verify_channel_members,
                          "models-people": verify_models_people, "reply-cut": verify_reply_cut,
                          "key-use": demo_key_use.verify_result,
                          "settings": verify_settings,
                          "task-page": lambda env: verify_task_page(env, restarter),
                          "previews": lambda env: demo_previews.verify_result(env, witness),
                          "titles": demo_titles.verify_result,
                          "catch-up": lambda env: verify_catch_up(env, configuration),
                          "agents": lambda env: demo_agents.verify_result(env, restarter),
                          "desktop": lambda env: demo_desktop.verify_result(env, restarter, pictures),
                          "basic": lambda env: (verify_workspace(env), demo_run_ids.verify(env))}
                failure = verify(args.suite, checks[args.suite], env)
            if failure is not None:
                # The sandboxes are gone by now; the services' logs remain until Compose takes the stack down.
                kept or keep_logs(env, failure, configuration)
        if witness is not None:
            witness.stop()
        compose.cleanup(directory, env, temporary, failure, compose=configuration)


if __name__ == "__main__":
    try:
        main()
    except (OSError, subprocess.SubprocessError):
        raise SystemExit("Demo contract failed; see diagnostics above") from None
