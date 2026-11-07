"""E2E-MCP-006: a task that needs an MCP-provided tool completes on the real stack (ADR-0011; MCP client slice 5).

The install's catalogue (`[[mcp.servers]]` in gateway.toml) defines one stdio server, `words`: the workstation image's own
python3 running a tiny MCP server given inline, so no image changes. A new conversation starts with it (enabled by
default), the gateway delivers it in session.config, and the runtime starts it inside the sandbox. The fake model calls
`mcp__words__count_words`; the journal records `mcp.words.count_words` and its result, and the run completes. The
`mcp.server` event reports it ready with its one tool before the run starts.

Run from the repository root: TQA_DOCKER_TESTS=1 python3 tests/e2e/mcp_task.py
(TQA_RUNTIME_TEST_IMAGE names the workstation runtime image; TQA_GATEWAY_IMAGE the gateway.)
"""

import json
import os
from pathlib import Path
import secrets
import signal
import socket
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "deploy" / "tests"))
import compose  # noqa: E402
import demo  # noqa: E402
import robustness  # noqa: E402
sys.path.insert(0, str(Path(__file__).resolve().parent))
from marker import unmarked  # noqa: E402

DEADLINE_SECONDS = 300
TEXT = "one two three four"
# A stdio MCP server small enough for one catalogue argument (at most 1 KiB): initialize, tools/list, tools/call. The
# same file is the catalogue's `words` entry (docs/catalog/mcp/words.md).
SERVER = (ROOT / "tests" / "e2e" / "servers" / "words.py").read_text()


class McpStack(robustness.Stack):
    def prepare(self, case="mcp"):
        image = self.env.get("TQA_GATEWAY_IMAGE", "tqa-gateway:local")
        print("Gateway image: " + image + " " + self.command(["docker", "image", "inspect", "--format", "{{.Id}}", image]),
              flush=True)
        runtime = demo.runtime_image(self.env, self.env.get("TQA_RUNTIME_TEST_IMAGE", "tqa-runtime:workstation"))
        print("Runtime image: " + runtime, flush=True)
        assert len(SERVER.encode()) <= 1024, "the inline server must fit one catalogue argument"
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
            compose.prepare(self.directory, self.env, runtime=runtime, port=port)
        compose.ownership(self.directory, self.env)
        self.origin = "http://127.0.0.1:" + str(port)
        replies = [
            {"events": [{"type": "tool_call", "name": "mcp__words__count_words", "split_into": 1,
                         "arguments": {"text": TEXT}}],
             "usage": {"prompt_tokens": 10, "completion_tokens": 10}},
            {"events": [{"type": "text", "text": "The text has four words."}],
             "usage": {"prompt_tokens": 20, "completion_tokens": 10}},
        ]
        compose.write(self.directory / "scenario.json", json.dumps({"model": "fixture", "responses": replies}))
        compose.write(self.directory.parent / "robustness.json", json.dumps({"services": {
            "fake-model": {"command": ["--scenario", "/scenarios/robustness.json"],
                           "volumes": [{"type": "bind", "source": str(self.directory / "scenario.json"),
                                        "target": "/scenarios/robustness.json", "read_only": True}]}}}))
        config = self.directory / "gateway.toml"
        catalogue = ("\n[[mcp.servers]]\nname='words'\ncommand='/usr/bin/python3'\nargs=" + json.dumps(["-c", SERVER])
                     + "\nenabled_by_default=true\ncall_timeout_seconds=30\n")
        compose.write(config, config.read_text() + catalogue)
        compose.ownership(self.directory, self.env, "1000:1000")
        self.up()

    def run(self):
        password = secrets.token_urlsafe(24)
        self.env["TQA_STACK_PASSWORD"] = password
        account = {"email": "mcp@example.invalid", "password": password}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.request("/api/v1/auth/login", account)
        conversation = self.request("/api/v1/conversations", {"title": "MCP task", "model_route": "local"})
        self.path = "/api/v1/conversations/" + conversation["id"]
        stream = self.request(self.path + "/events?stream=1&protocol=1", after=0)
        self.request(self.path + "/messages", {"text": "Count the words in: " + TEXT, "attachments": []})
        stream.until("run.completed")
        return self.request(self.path + "/events?after=0&limit=1000")["items"]


def verify(stored):
    events = [frame["event"] for frame in stored if frame.get("type") == "event"]
    kinds = [event["type"] for event in events]
    servers = [event for event in events if event["type"] == "mcp.server"]
    assert servers == [{"type": "mcp.server", "name": "words", "state": "ready", "tools": 1}], servers
    assert kinds.index("mcp.server") < kinds.index("run.started"), "the server is ready before the run starts"
    calls = [event for event in events if event["type"] == "tool.call"]
    assert [call["name"] for call in calls] == ["mcp.words.count_words"], calls
    assert calls[0]["arguments"] == {"text": TEXT}
    results = [event for event in events if event["type"] == "tool.result"]
    assert len(results) == 1 and results[0]["status"] == "ok", results
    # The server's answer comes back as one untrusted-content block (SEC-040).
    answer = unmarked(results[0]["content"], "words", "count_words").strip()
    assert answer == "words: 4", results[0]
    assert kinds[-1] == "run.completed", kinds[-5:]
    print("E2E-MCP-006 passed: mcp.words.count_words answered '{}', marked, and the run completed".format(answer),
          flush=True)


def interrupted(_number, _frame):
    raise InterruptedError("E2E-MCP-006 was interrupted or exceeded its deadline")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-mcp-")
    directory = compose.installation(temporary)
    stack, failure = McpStack(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(DEADLINE_SECONDS)
    try:
        stack.prepare()
        verify(stack.run())
    except BaseException as error:
        failure = error
        print("E2E-MCP-006 failed: " + compose.redact(str(error), env), flush=True)
        try:
            logs = stack.command(stack.configuration + ["logs", "--no-color", "gateway"])
            print(compose.redact(logs[-6000:], env), flush=True)
        except Exception as diagnostic:
            print("Gateway logs unavailable: " + compose.redact(str(diagnostic), env), flush=True)
    finally:
        signal.alarm(0)
        for stream in stack.streams:
            stream.response.close()
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
