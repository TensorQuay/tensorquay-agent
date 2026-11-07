"""E2E-PROFILE-007: a task run under a restricted profile with one connector completes using only the allowed tools
(ADR-0012, ADR-0031; protocol revision 8).

The stack enables the catalogue server `words` and the connector `tracker` by default. `tracker` is a small MCP server
on the standard library, run from the workstation runtime image on the gateway's control network, so no image changes.
The owner makes a personal profile that allows `read_file` and `tracker` only, then a conversation from it. The fake
model calls `shell` (refused as an unknown tool, naming only what the profile allows), then `tracker`'s `lookup`, then
answers. See profile_task_evidence.py for the checks.

Run from the repository root: TQA_DOCKER_TESTS=1 python3 tests/e2e/profile_task.py
(TQA_RUNTIME_TEST_IMAGE names the workstation runtime image, at revision 8 or later; TQA_GATEWAY_IMAGE the gateway.)
"""

import json
import os
from pathlib import Path
import re
import secrets
import signal
import socket
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "deploy" / "tests"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import compose  # noqa: E402
import demo  # noqa: E402
import mcp_task  # noqa: E402
import profile_task_evidence as evidence  # noqa: E402
import robustness  # noqa: E402

DEADLINE_SECONDS = 300
CONNECTOR_START_SECONDS = 30


class ProfileStack(robustness.Stack):
    def prepare(self, case="profile"):
        image = self.env.get("TQA_GATEWAY_IMAGE", "tqa-gateway:local")
        print("Gateway image: " + image + " " + self.command(["docker", "image", "inspect", "--format", "{{.Id}}", image]),
              flush=True)
        workstation = self.env.get("TQA_RUNTIME_TEST_IMAGE", "tqa-runtime:workstation")
        runtime = demo.runtime_image(self.env, workstation)
        print("Runtime image: " + runtime, flush=True)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
            compose.prepare(self.directory, self.env, runtime=runtime, port=port)
        compose.ownership(self.directory, self.env)
        self.origin = "http://127.0.0.1:" + str(port)
        compose.write(self.directory / "scenario.json", json.dumps(evidence.replies()))
        compose.write(self.directory.parent / "robustness.json", json.dumps({"services": {
            "fake-model": {"command": ["--scenario", "/scenarios/robustness.json"],
                           "volumes": [{"type": "bind", "source": str(self.directory / "scenario.json"),
                                        "target": "/scenarios/robustness.json", "read_only": True}]},
            "connector": {"image": workstation, "pull_policy": "never", "user": "1000:1000",
                          "entrypoint": ["/usr/bin/python3", "-c", evidence.CONNECTOR, str(evidence.CONNECTOR_PORT)],
                          "networks": ["control"], "read_only": True, "cap_drop": ["ALL"],
                          "security_opt": ["no-new-privileges:true"], "mem_limit": "64m", "pids_limit": 32}}}))
        # The connector first: the gateway's file names it by its address, which exists only once it runs.
        self.command(self.configuration + ["up", "-d", "--no-build", "--no-deps", "connector"])
        address = self.connector_address()
        print("Connector address: " + address, flush=True)
        config = self.directory / "gateway.toml"
        compose.write(config, config.read_text() + evidence.gateway_config(address, mcp_task.SERVER))
        compose.ownership(self.directory, self.env, "1000:1000")
        self.up()

    def connector_address(self):
        deadline = time.monotonic() + CONNECTOR_START_SECONDS
        while "listening" not in self.command(self.configuration + ["logs", "--no-color", "connector"]):
            if time.monotonic() > deadline:
                raise TimeoutError("the connector did not start listening")
            time.sleep(0.5)
        identity = self.command(self.configuration + ["ps", "-q", "connector"])
        if not re.fullmatch(r"[0-9a-f]{12,64}", identity):
            raise ValueError("Unexpected connector container identifier")
        addresses = self.command(["docker", "inspect", "--format",
                                  "{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}", identity]).split()
        if len(addresses) != 1:
            raise ValueError("the connector must be on exactly one network: " + " ".join(addresses))
        return addresses[0]

    def run(self):
        password = secrets.token_urlsafe(24)
        self.env["TQA_STACK_PASSWORD"] = password
        account = {"email": "profile@example.invalid", "password": password}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.request("/api/v1/auth/login", account)
        profile = self.request("/api/v1/profiles", evidence.profile())
        assert profile.get("usable") is True, profile
        conversation = self.request("/api/v1/conversations", {"title": "Profile task", "profile": profile["id"]})
        self.path = "/api/v1/conversations/" + conversation["id"]
        stream = self.request(self.path + "/events?stream=1&protocol=1", after=0)
        self.request(self.path + "/messages", {"text": "What is the state of issue T-1?", "attachments": []})
        stream.until("run.completed")
        return conversation, self.request(self.path + "/events?after=0&limit=1000")["items"]


def interrupted(_number, _frame):
    raise InterruptedError("E2E-PROFILE-007 was interrupted or exceeded its deadline")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-profile-")
    directory = compose.installation(temporary)
    stack, failure = ProfileStack(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(DEADLINE_SECONDS)
    try:
        stack.prepare()
        observed = evidence.verify(*stack.run())
        print("E2E-PROFILE-007 passed: " + json.dumps(observed), flush=True)
    except BaseException as error:
        failure = error
        print("E2E-PROFILE-007 failed: " + compose.redact(str(error), env), flush=True)
        for service in ("gateway", "connector"):
            try:
                logs = stack.command(stack.configuration + ["logs", "--no-color", service])
                print(compose.redact(logs[-6000:], env), flush=True)
            except Exception as diagnostic:
                print(service + " logs unavailable: " + compose.redact(str(diagnostic), env), flush=True)
    finally:
        signal.alarm(0)
        # A second interrupt, or a runner's TERM, must not skip the teardown that follows.
        for number in (signal.SIGINT, signal.SIGTERM):
            signal.signal(number, signal.SIG_IGN)
        for stream in stack.streams:
            stream.response.close()
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
