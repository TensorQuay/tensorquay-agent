"""E2E-PREVIEW-BUILT-001: a web app the agent builds previews as it is, on a real model on the real stack (18.1).

Each run is one task on the Workstation with the package registries allowed (npm needs registry.npmjs.org). The task
asks for a small React app in app/: a home page and an About page reached by a link, a logo imported in JS, a
stylesheet with a background url(), and About loaded lazily; then it asks to build it. A row's task text says what
to build, or (row "guided") nothing about paths or routers, which measures the prompt's web-apps line.

The driver reads what the model actually wrote: the Vite base or CRA homepage, the router, and the built index.html's
asset paths, and whether every asset the built page names exists beside it. It also records which index.html the
answer named. A guided run passes when the model chose a relative base and a hash router and the build's page names
only relative assets that exist. Other rows record the same facts for the preview checks.

Usage (the soak's real-model settings and the images under test, from the repository's root):
  TQA_DOCKER_TESTS=1 TQA_SOAK_MODEL_URL=... TQA_SOAK_MODEL_NAME=... TQA_SOAK_MODEL_KEY_FILE=... \\
  TQA_GATEWAY_IMAGE=<id> TQA_WEB_IMAGE=<id> TQA_RUNTIME_IMAGE=<id> TQA_WORKSTATION_IMAGE=<id> \\
  TQA_FAKE_MODEL_IMAGE=<id> python3 deploy/tests/built_apps_e2e.py --output DIR --row guided [--runs 5]
  python3 deploy/tests/built_apps_e2e.py --check-fixture DIR   # the verdicts on a folder already on disk
"""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import posixpath
import re
import sys
import tempfile
import time
import urllib.parse

sys.path.insert(0, str(Path(__file__).resolve().parent))

APP = "app"
TURN_SECONDS = 1500  # an npm install and a build on a loaded engine
BASE_TASK = ("Make a small React web app in the folder app/ with Vite: a home page and an About page reached by a "
             "link, a logo image imported in the JavaScript, a stylesheet with a background url() image, and the "
             "About page loaded lazily. {extra}Install what it needs, build it, and tell me which index.html to open "
             "to see it.")
ROWS = {
    "guided": "",  # nothing about paths or routers: the prompt's line is measured
    "vite-default": "Keep Vite's default config. ",
    "vite-relative": "Use base './' in the Vite config and a HashRouter. ",
    "vite-browser-router": "Use BrowserRouter for the pages. ",
}
GUIDED_PASS_OF = (4, 5)  # the design's bar for the guided row
ASSET = re.compile(r"""<(?:script|link|img)\b[^>]*?\b(?:src|href)\s*=\s*["']([^"']+)["']""", re.I)
ROOT = re.compile(r"^/(?![/\\])")


def router_kind(sources):
    """The router the app's sources use: hash, browser, or none."""
    text = "\n".join(sources)
    if re.search(r"\b(createHashRouter|HashRouter)\b", text):
        return "hash"
    if re.search(r"\b(createBrowserRouter|BrowserRouter)\b", text):
        return "browser"
    return "none"


def vite_base(config):
    """The base a Vite config sets: its value, or None when it sets none (Vite's default is '/')."""
    match = re.search(r"""\bbase\s*:\s*(["'`])([^"'`]*)\1""", config or "")
    return match.group(2) if match else None


def relative_base(base, homepage):
    return (base is not None and not base.startswith("/")) or (homepage is not None and not homepage.startswith("/")
                                                                and "://" not in homepage)


def page_assets(html):
    """The script, stylesheet and image addresses a built page names (absolute URLs and data: left out)."""
    found = []
    for url in ASSET.findall(html or ""):
        if re.match(r"^[a-z][a-z0-9+.-]*:", url, re.I) or url.startswith("//") or url.startswith("#"):
            continue
        found.append(url)
    return found


def verdict(files, answer):
    """The facts of one run from the files it left: {path: text or None}; the folder of the built page and the rest."""
    config = next((files[p] for p in sorted(files) if re.fullmatch(APP + r"/vite\.config\.(js|mjs|cjs|ts|mts)", p)),
                  None)
    package = files.get(APP + "/package.json")
    try:
        homepage = json.loads(package).get("homepage") if package else None
    except ValueError:
        homepage = None
    sources = [text for path, text in files.items() if path.startswith(APP + "/src/") and text]
    built = next((p for p in (APP + "/dist/index.html", APP + "/build/index.html") if files.get(p)), None)
    assets = page_assets(files.get(built)) if built else []
    folder = posixpath.dirname(built) if built else None
    missing = [url for url in assets if not ROOT.match(url)
               and posixpath.normpath(posixpath.join(folder, urllib.parse.unquote(url.split("?")[0]))) not in files]
    base = vite_base(config)
    record = {
        "base": base, "homepage": homepage, "router": router_kind(sources), "built": built,
        "assets": assets[:10], "root_assets": [url for url in assets if ROOT.match(url)][:10], "missing": missing[:10],
        "named_index": bool(re.search(r"(dist|build)/index\.html", answer or "")),
    }
    record["relative_base"] = relative_base(base, homepage)
    record["previews"] = bool(built) and bool(assets) and not record["root_assets"] and not missing
    record["guided_pass"] = record["relative_base"] and record["router"] == "hash" and record["previews"]
    return record


def check_fixture(folder):
    """The verdict on a folder already on disk (a hand-built app), to prove the checks pass before a real run."""
    root = Path(folder)
    files = {}
    for path in root.rglob("*"):
        if path.is_file() and "node_modules" not in path.parts:
            relative = path.relative_to(root).as_posix()
            try:
                files[relative] = path.read_text(encoding="utf-8")
            except UnicodeDecodeError:
                files[relative] = None  # binary: present, not read
    return verdict(files, "Open app/dist/index.html.")


# ---- the real stack -------------------------------------------------------------------------------------------------

def events_of(stack, conversation):
    return [item.get("event") or {} for page in stack.stored(conversation) for item in page]


def send(stack, conversation, text):
    """Sends a message and waits for its run to end; returns the task's events."""
    ends = ("run.completed", "run.failed", "run.cancelled")
    before = len([e for e in events_of(stack, conversation) if e.get("type") in ends])
    stack.request("/api/v1/conversations/{}/messages".format(conversation), {"text": text})
    deadline = time.monotonic() + TURN_SECONDS
    while time.monotonic() < deadline:
        events = events_of(stack, conversation)
        if len([e for e in events if e.get("type") in ends]) > before:
            return events
        time.sleep(5)
    raise TimeoutError("the turn did not end within {} s".format(TURN_SECONDS))


def listing(stack, conversation, path):
    items, after = [], ""
    for _ in range(20):
        page = stack.request("/api/v1/conversations/{}/files?limit=200&path={}{}".format(
            conversation, urllib.parse.quote(path), "&after=" + urllib.parse.quote(after) if after else ""))
        items += page.get("items") or []
        after = page.get("next_after")
        if not after:
            break
    return items


def workspace_files(stack, conversation, root=APP, depth=4):
    """{path: text or None} under root, without node_modules; texts read through the files API."""
    files, folders = {}, [(root, 0)]
    while folders:
        folder, level = folders.pop()
        for item in listing(stack, conversation, folder):
            path = posixpath.join(folder, item["name"])
            if item.get("kind") == "directory":
                if item["name"] != "node_modules" and level < depth:
                    folders.append((path, level + 1))
                continue
            content = stack.request("/api/v1/conversations/{}/files/content?path={}".format(
                conversation, urllib.parse.quote(path)))
            files[path] = None if content.get("binary") else content.get("text")
    return files


def one_run(stack, row, index, output):
    record = {"row": row, "run": index}
    try:
        conversation = stack.request("/api/v1/conversations", {"title": "Built app {} {}".format(row, index),
                                                               "model_route": "local", "template": "workstation"})["id"]
        record["conversation"] = conversation
        events = send(stack, conversation, BASE_TASK.format(extra=ROWS[row]))
        answer = "".join(e.get("text", "") for e in events if e.get("type") == "text.delta")
        record.update(verdict(workspace_files(stack, conversation), answer))
        record["completed"] = any(e.get("type") == "run.completed" for e in events)
        record["answer_tail"] = answer.strip()[-300:]
        (output / "run-{}-{}.json".format(row, index)).write_text(json.dumps(events, indent=1) + "\n")
    except Exception as error:  # a run that breaks is a failed run, kept with why
        record.update(error="{}: {}".format(type(error).__name__, str(error)[:300]), guided_pass=False)
    print("== {} {} run {}: {}".format(time.strftime("%H:%M:%S"), row, index, json.dumps(
        {k: record.get(k) for k in ("base", "homepage", "router", "previews", "root_assets", "missing", "error")})),
        flush=True)
    return record


def main(argv=None):
    parser = argparse.ArgumentParser(description="E2E-PREVIEW-BUILT-001 on a real model")
    parser.add_argument("--output")
    parser.add_argument("--row", choices=sorted(ROWS), default="guided")
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--parallel", type=int, default=2)
    parser.add_argument("--check-fixture", metavar="DIR")
    options = parser.parse_args(argv)
    if options.check_fixture:
        print(json.dumps(check_fixture(options.check_fixture), indent=1))
        return 0
    if not options.output:
        parser.error("--output is needed for a run")
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("E2E-PREVIEW-BUILT-001 needs Docker: set TQA_DOCKER_TESTS=1")
    import compose  # noqa: E402 (only a real run needs the stack)
    from soak_stack import Stack, cleanup_actions  # noqa: E402
    compose.teardown_on_signals()
    output = Path(options.output)
    output.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    temporary = tempfile.TemporaryDirectory(prefix="tqa-built-apps-")
    directory = Path(temporary.name) / "installation"
    directory.mkdir(mode=0o700)
    stack, failure, records = Stack(directory, env, output, model="real"), None, []
    try:
        stack.prepare(rehearsal=True)
        stack.request("/api/v1/admin/egress/presets/package_registries", {})
        with concurrent.futures.ThreadPoolExecutor(max_workers=options.parallel) as pool:
            records = list(pool.map(lambda index: one_run(stack, options.row, index, output),
                                    range(1, options.runs + 1)))
    except BaseException as error:
        failure = error
    finally:
        failure = cleanup_actions([stack.save_logs], failure)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)
    passed = sum(1 for record in records if record.get("guided_pass"))
    pass_at = -(-len(records) * GUIDED_PASS_OF[0] // GUIDED_PASS_OF[1])
    result = {"id": "E2E-PREVIEW-BUILT-001", "row": options.row, "runs": len(records), "guided_passed": passed,
              "pass_at": pass_at if options.row == "guided" else None, "model": env.get("TQA_SOAK_MODEL_NAME"),
              "failure": None if failure is None else str(failure)[:500], "records": records}
    result["status"] = ("passed" if failure is None and records and (options.row != "guided" or passed >= pass_at)
                        else "failed")
    (output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print("E2E-PREVIEW-BUILT-001 {} ({}): {} of {} chose a relative base and a hash router and previewed".format(
        result["status"], options.row, passed, len(records)), flush=True)
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
