"""PROMPT-U-021's plumbing on the real stack (SBX-C-021): the web-search hint's own commands work through the proxy.

The owned egress stack (stack.py: the gateway, its proxy and allow-list, the Workstation runtime image, the scripted
model) with three public rules, `en.wikipedia.org`, `html.duckduckgo.com` and `example.com`, all on 443. The scripted
model makes the hint's calls, as a model following it would:
  0. the Facts line's curl on Wikipedia's search API, once (version 14): JSON, small;
  1. the hint's curl on the search page, once;
  2. the hint's Chromium (`--headless --no-sandbox --proxy-server=${HTTPS_PROXY%/} --dump-dom`): on the search page only
     if curl's page held no results (the hint's own fallback, so still one query), otherwise on example.com;
  3. `fetch_url` on example.com, whose result must arrive marked untrusted;
  4. `fetch_url` on a host off the list, the control, refused in plain words.
The search site may answer an automated client with its own challenge page: any HTTP answer proves the path, so the
check is on the plumbing, never on the results. One query a run and no retries (the technical lead, 7 Oct 2026). The sandbox has no
route but the proxy (SBX-C-004), so an answer at all means the proxy carried it.

Run from the repository root (public network; Docker):
    PYTHONPATH=crates/tqa-sandbox/tests TQA_DOCKER_TESTS=1 TQA_PUBLIC_NETWORK_TESTS=1 \\
    TQA_GATEWAY_IMAGE=<gateway image id> TQA_WEB_IMAGE=<web image id> \\
    TQA_RUNTIME_TEST_IMAGE=<workstation runtime image> [TQA_SEARCH_EVIDENCE=<directory>] python3 -m egress.web_search
"""
import json
import os
from pathlib import Path
import re
import secrets
import signal
import tempfile
import time

from .stack import Stack, compose

FACTS_SEARCH = ("https://en.wikipedia.org/w/api.php?action=query&list=search&format=json&srlimit=5"
                "&srsearch=nodejs+lts+release")
# The Facts line's answer is titles and snippets, never a page of markup (a 32k model's context): this much at most.
FACTS_MOST_BYTES = 20_000
SEARCH = "https://html.duckduckgo.com/html/?q=nodejs+lts+release"
PAGE = "https://example.com/"
DENIED = "https://denied.invalid/"
# The hint's commands, word for word in their flags (PROMPT-U-021); the outputs go to files the check reads, and each
# command prints its exit status so the shell call itself succeeds and the check decides.
FACTS = ("curl -s '" + FACTS_SEARCH + "' -o facts.json -w 'http %{http_code}\\n'; echo \"curl exit $?\"; "
         "wc -c < facts.json")
CURL = "curl -s '" + SEARCH + "' -o search.html -w 'http %{http_code}\\n'; echo \"curl exit $?\"; wc -c < search.html"
CHROMIUM = ("if grep -q 'result__a' search.html; then url='" + PAGE + "'; else url='" + SEARCH + "'; fi; "
            "chromium --headless --no-sandbox --proxy-server=${HTTPS_PROXY%/} --dump-dom \"$url\" > dom.html 2> chromium.err; "
            "echo \"chromium exit $?\"; echo \"target $url\"; wc -c < dom.html")
DONE = "search-plumbing-done"
RUN_SECONDS = 240


class SearchStack(Stack):
    extra_allow = (("en.wikipedia.org", [443]), ("html.duckduckgo.com", [443]), ("example.com", [443]))

    def scenario(self):
        calls = [("shell", {"command": FACTS}), ("shell", {"command": CURL}), ("shell", {"command": CHROMIUM}),
                 ("fetch_url", {"url": PAGE}), ("fetch_url", {"url": DENIED})]
        responses = [{"events": [{"type": "tool_call", "name": name, "arguments": args, "split_into": 1}]}
                     for name, args in calls]
        responses += [{"events": [{"type": "text", "text": DONE}]}, {"events": [{"type": "text", "text": DONE}]}]
        for response in responses:
            response["usage"] = {"prompt_tokens": 10, "completion_tokens": 10}
        compose.write(self.directory / "scenario.json", json.dumps({"model": "fixture", "responses": responses}))

    def start(self):
        password = "synthetic-search-" + secrets.token_hex(16)
        self.env["TQA_STACK_PASSWORD"] = password
        account = {"email": "search@example.invalid", "password": password}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.request("/api/v1/auth/login", account)
        conversation = self.request("/api/v1/conversations", {"title": "Web search check", "model_route": "local"})
        self.conversation = conversation["id"]
        self.path = "/api/v1/conversations/" + self.conversation
        stream = self.request(self.path + "/events?stream=1&protocol=1", after=0)
        self.request(self.path + "/messages", {"text": "What is the current Node.js LTS release? Search the web.",
                                               "attachments": []})
        return stream

    def workspace_file(self, name, limit=200_000):
        """A file the run wrote in its workspace, as text (at most `limit` characters), or "" when there is none."""
        compose.ownership(self.directory, self.env)
        try:
            path = Path(self.env["TQA_STORAGE_ROOT"]) / "workspaces" / self.conversation / name
            return path.read_text(errors="replace")[:limit] if path.exists() else ""
        finally:
            compose.ownership(self.directory, self.env, "1000:1000")


def finished(stream):
    """Waits for the run's end; a failed run fails the check with its reason."""
    deadline = time.monotonic() + RUN_SECONDS
    while time.monotonic() < deadline:
        event = stream.next().get("event", {})
        assert event.get("type") != "run.failed", "The scripted search run failed: {}".format(event.get("reason"))
        if event.get("type") == "run.completed":
            return
    raise AssertionError("The scripted search run did not complete in {} s".format(RUN_SECONDS))


def check(stack, stream, evidence):
    """The five calls' durable results, as described above. `evidence` fills as each is read, so a failure keeps what
    was seen up to it (the owner's log needs the why, not only the assertion)."""
    finished(stream)
    frames = stack.request(stack.path + "/events?after=0&limit=1000")["items"]
    evidence["events"] = frames
    results = [frame["event"] for frame in frames if frame.get("event", {}).get("type") == "tool.result"]
    evidence["results"] = [{key: result.get(key) for key in ("status", "content")} for result in results]
    for name, limit in (("facts.json", 400_000), ("search.html", 200_000), ("dom.html", 200_000), ("chromium.err", 200_000)):
        evidence[name] = stack.workspace_file(name, limit)
    assert len(results) == 5, "Every scripted call must have one durable result: {}".format(len(results))
    facts, curl, chromium, page, denied = results
    # Wikipedia's search (the Facts line): an HTTP answer through the proxy, and how many results it gave (an exact
    # title's article counts as one).
    facts_code = re.search(r"http (\d{3})", facts["content"])
    evidence["facts_status"] = facts_code.group(1) if facts_code else None
    evidence["facts_bytes"] = len(evidence["facts.json"].encode())
    try:
        found = json.loads(evidence["facts.json"])["query"]["search"]
        evidence["facts_results"] = len(found)
        evidence["facts_titles"] = [hit.get("title") for hit in found]
    except (ValueError, KeyError, TypeError):
        evidence["facts_results"] = None
    assert facts["status"] == "ok" and "curl exit 0" in facts["content"], facts
    assert facts_code and facts_code.group(1) != "000", "Wikipedia gave no HTTP answer: " + facts["content"][-500:]
    assert evidence["facts_bytes"] <= FACTS_MOST_BYTES, "Wikipedia's answer is {} bytes".format(evidence["facts_bytes"])
    search_page = evidence["search.html"]
    code = re.search(r"http (\d{3})", curl["content"])
    evidence["search_status"] = code.group(1) if code else None
    assert curl["status"] == "ok" and "curl exit 0" in curl["content"], curl
    assert code and code.group(1) != "000", "curl got no HTTP answer through the proxy: " + curl["content"][-500:]
    kind = "results" if "result__a" in search_page else "the site's own page (no results; a challenge, say)"
    evidence["search_page"] = kind
    target = re.search(r"target (\S+)", chromium["content"])
    evidence["chromium_target"] = target.group(1) if target else None
    assert chromium["status"] == "ok" and "chromium exit 0" in chromium["content"], chromium
    assert re.search(r"<html", evidence["dom.html"], re.IGNORECASE), (
        "Chromium dumped no page ({}): {}".format(chromium["content"][-300:], chromium_reason(evidence["chromium.err"])))
    assert page["status"] == "ok", page
    # The page arrives inside the untrusted block with some text of its own; never a particular wording, since the site's
    # changes (7 Oct: example.com's text no longer starts "Example Domain").
    block = re.search(r"<untrusted-content[^>]*>(.*?)</untrusted-content", page["content"], re.DOTALL)
    assert block and block.group(1).strip(), page["content"][:500]
    assert denied["status"] == "error", denied
    assert denied["content"].startswith("denied.invalid isn't on this task's allowed sites."), denied
    print("SBX-C-021: Wikipedia answered {} with {} results in {} bytes; the search page answered {} ({}); Chromium loaded {}; "
          "fetch_url's page marked untrusted; the host off the list refused".format(
              facts_code.group(1), evidence["facts_results"], evidence["facts_bytes"], code.group(1), kind, evidence["chromium_target"] or "?"),
          flush=True)


def chromium_reason(err, lines=12):
    """The end of Chromium's stderr without its D-Bus lines (a container has no system bus, and every start says so)."""
    kept = [line for line in err.splitlines() if "dbus" not in line.lower()]
    return "\n".join(kept[-lines:]) or "(nothing but D-Bus noise)"


def keep(evidence, directory, error=None):
    """The run's evidence, for the owner's log, whether it passed or not: the events and tool results, the pages, and
    a summary with the verdict."""
    directory.mkdir(parents=True, exist_ok=True)
    for name in ("facts.json", "search.html", "dom.html", "chromium.err"):
        if name in evidence:
            (directory / name).write_text(evidence[name])
    for name in ("events", "results"):
        if name in evidence:
            (directory / (name + ".json")).write_text(json.dumps(evidence[name], indent=1))
    summary = {key: evidence.get(key) for key in ("facts_status", "facts_bytes", "facts_results", "facts_titles",
                                                   "search_status", "search_page", "chromium_target")}
    summary["verdict"] = "passed" if error is None else "failed"
    if error is not None:
        summary["error"] = error
    (directory / "summary.json").write_text(json.dumps(summary, indent=1))


def interrupted(_number, _frame):
    raise InterruptedError("The search check was interrupted or exceeded its eight-minute deadline")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    if os.environ.get("TQA_PUBLIC_NETWORK_TESTS") != "1":
        raise SystemExit("The search check reaches public sites: it requires TQA_PUBLIC_NETWORK_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-search-")
    directory = compose.installation(temporary)
    stack, failure, evidence = SearchStack(directory, env), None, {}
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(480)
    try:
        stack.prepare("search")
        check(stack, stack.start(), evidence)
        if env.get("TQA_SEARCH_EVIDENCE"):
            keep(evidence, Path(env["TQA_SEARCH_EVIDENCE"]))
    except BaseException as error:
        failure = error
        reason = compose.redact(str(error), env)
        print("Search check failed: " + reason, flush=True)
        if env.get("TQA_SEARCH_EVIDENCE"):
            keep(evidence, Path(env["TQA_SEARCH_EVIDENCE"]), error=reason)
        stack.diagnostics()
    finally:
        signal.alarm(0)
        for stream in stack.streams:
            stream.response.close()
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
