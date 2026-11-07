"""E2E-CHANNEL-016: the Gate 7 demo as an automated scenario (DEV-PLAN Phase 7; ADR-0014).

Two agents (Planner, Builder) and a person on one channel: both agents propose, the person votes and the decision is
posted, the person vetoes the other proposal, the winning agent claims the task and reports, the person claims the
second place and reports, and the Planner closes with `done`. The script, the fixed seqs and the checks (C1-C5) are in
gate7_evidence.py; the agents' scenarios, filled in with the channel's id, in gate7_scenarios.py.

The agents are real runs in sandboxes: each profile allows only the four channel tools, on its own route to its own
fake-model service. The person acts either over the API (`--person api`, the default) or from the channel page
(`--person page`: the WEB-E2E-012 spec, web/e2e/channel-stack.spec.ts, as TQA_STACK_SUITE=channels).

Run from the repository root, with no other stack running on the host:
TQA_DOCKER_TESTS=1 python3 tests/e2e/gate7.py [--person api|page]
(TQA_RUNTIME_TEST_IMAGE names the workstation runtime image with the channel tools; TQA_GATEWAY_IMAGE the gateway.)
"""

import argparse
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "deploy" / "tests"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import compose  # noqa: E402
import demo  # noqa: E402
import gate7_evidence as evidence  # noqa: E402
import robustness  # noqa: E402

DEADLINE_SECONDS = 1800
# Each agent step: a model turn and, the first time, a sandbox start.
STEP_SECONDS = 240
# The page spec allows itself 20 minutes (180 s per agent step, and signing in); the driver gives it more, so a slow
# step fails in the spec, which names it, rather than here.
PAGE_SECONDS = 1320
FAKE_MODEL_IMAGE = "tqa-fake-model:local"
# Until the channel exists, each fake model serves a placeholder; the real scenario needs the channel's id.
PLACEHOLDER = {"model": "fixture", "responses": [{"events": [{"type": "text", "text": "waiting for the channel"}],
                                                  "usage": {"prompt_tokens": 1, "completion_tokens": 1}}]}


class ChannelStack(robustness.Stack):
    def prepare(self, case="channels"):
        image = self.env.get("TQA_GATEWAY_IMAGE", "tqa-gateway:local")
        print("Gateway image: " + image + " " + self.command(["docker", "image", "inspect", "--format", "{{.Id}}", image]),
              flush=True)
        runtime = demo.runtime_image(self.env, self.env.get("TQA_RUNTIME_TEST_IMAGE", "tqa-runtime:workstation"))
        print("Runtime image: " + runtime, flush=True)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
            compose.prepare(self.directory, self.env, runtime=runtime, port=port)
        compose.ownership(self.directory, self.env)
        self.origin = "http://127.0.0.1:" + str(port)
        models = self.directory / "models.toml"
        compose.write(models, models.read_text() + "".join(
            "\n[[routes]]\nname='{}'\nprotocol='openai_chat'\nbase_url='http://fake-{}:8080/v1'\nmodel='fixture'\n"
            "auth={{kind='bearer'}}\nkey_ref='secrets/local'\n".format(route, route) for route in evidence.ROUTES.values()))
        services = {}
        for route in evidence.ROUTES.values():
            scenario = self.directory / (route + ".json")
            compose.write(scenario, json.dumps(PLACEHOLDER))
            services["fake-" + route] = {
                "image": self.env.get("TQA_FAKE_MODEL_IMAGE", FAKE_MODEL_IMAGE), "pull_policy": "never",
                "command": ["--scenario", "/scenarios/agent.json"], "user": "1000:1000", "networks": ["control"],
                "volumes": [{"type": "bind", "source": str(scenario), "target": "/scenarios/agent.json",
                             "read_only": True}],
                "read_only": True, "cap_drop": ["ALL"], "security_opt": ["no-new-privileges:true"],
                "mem_limit": "64m", "pids_limit": 32}
        compose.write(self.directory.parent / "robustness.json", json.dumps({"services": services}))
        compose.ownership(self.directory, self.env, "1000:1000")
        self.up()

    def request(self, path, body=None, after=None):
        """robustness.Stack's request, with a refusal's error body in the failure (it names the refused field)."""
        try:
            return super().request(path, body, after)
        except urllib.error.HTTPError as error:
            raise AssertionError(evidence.refusal(path, error.code, error.read())) from None

    def call(self, method, path, body=None):
        """A request robustness.Stack lacks (PUT, a POST without a body); the parsed answer, or {} when empty."""
        request = urllib.request.Request(self.origin + path, method=method,
                                         data=None if body is None else json.dumps(body).encode(),
                                         headers={"Origin": self.origin, "Content-Type": "application/json",
                                                  "Idempotency-Key": secrets.token_hex(16)})
        try:
            with self.client.open(request, timeout=15) as response:
                text = response.read(1048577).decode()
        except urllib.error.HTTPError as error:
            raise AssertionError(evidence.refusal(path, error.code, error.read())) from None
        return json.loads(text) if text else {}

    def setup(self):
        """The person, the two agents' profiles and conversations, the channel and its members; the scenarios."""
        password = secrets.token_urlsafe(24)
        self.env["TQA_STACK_PASSWORD"] = password
        account = {"email": evidence.PERSON_EMAIL, "password": password}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.request("/api/v1/auth/login", account)
        self.agents = {}
        for agent in evidence.AGENTS:
            profile = self.request("/api/v1/profiles", evidence.profile(agent))
            assert profile.get("usable") is True, profile
            conversation = self.request("/api/v1/conversations", {"title": agent, "profile": profile["id"]})
            self.agents[agent] = conversation["id"]
        channel = self.request("/api/v1/channels", {"name": evidence.CHANNEL_NAME, "rule": "majority",
                                                    "purpose": "Pick an approach and do the work."})
        self.channel = channel["id"]
        for agent, conversation in self.agents.items():
            self.call("PUT", "/api/v1/channels/{}/members/conversation/{}".format(self.channel, conversation),
                      {"role": "member"})
        import gate7_scenarios  # noqa: PLC0415  (the E2E-CHANNEL-016 scenario templates, filled in with the channel's id)
        # The installation belongs to the gateway user by now: its owner rewrites the files, in a container.
        for agent, scenario in gate7_scenarios.scenarios(self.channel).items():
            path = self.directory / (evidence.ROUTES[agent] + ".json")
            subprocess.run(evidence.scenario_writer(path, compose.TOYBOX), input=json.dumps(scenario).encode(),
                           env=self.env, capture_output=True, timeout=60, check=True)
        self.command(self.configuration + ["restart"] + ["fake-" + route for route in evidence.ROUTES.values()])
        print("Channel {} with Planner {} and Builder {}".format(self.channel, self.agents["Planner"],
                                                                 self.agents["Builder"]), flush=True)

    def start_agents(self):
        for agent, conversation in self.agents.items():
            self.request("/api/v1/conversations/{}/messages".format(conversation),
                         {"text": "You are {} on channel {}. Follow the channel.".format(agent, self.channel),
                          "attachments": []})

    def until_seq(self, seq):
        """Waits until the channel's log reaches `seq` (an agent's step the person answers)."""
        deadline = time.monotonic() + STEP_SECONDS
        while time.monotonic() < deadline:
            page = self.request("/api/v1/channels/{}/messages?since={}&limit=1".format(self.channel, seq - 1))
            if page["messages"]:
                return page["messages"][0]
            time.sleep(1)
        raise AssertionError("the channel did not reach seq {} in {} s".format(seq, STEP_SECONDS))

    def person_by_api(self):
        """The person's steps over the API, each after the agent step it answers (see gate7_evidence.LOG)."""
        post = "/api/v1/channels/{}/messages".format(self.channel)
        self.until_seq(4)
        self.request(post, {"body": {"type": "vote", "proposal_seq": 2, "choice": "accept"}})
        self.until_seq(6)
        self.request(post, {"body": {"type": "vote", "proposal_seq": 3, "choice": "veto"}})
        self.until_seq(11)
        self.call("POST", "/api/v1/channels/{}/tasks/9/claim".format(self.channel))
        self.request(post, {"body": {"type": "report", "task_seq": 9, "status": "completed",
                                     "summary": evidence.SCRIPT["person_report"]}})
        return None

    def person_by_page(self):
        """The WEB-E2E-012 spec is the person; its result file holds the seqs it saw."""
        script, result = evidence.page_files(self.directory)
        compose.write(script, evidence.script_file())
        env = dict(self.env, TQA_STACK_SUITE="channels", TQA_STACK_URL=self.origin, TQA_CHANNEL_ID=self.channel,
                   TQA_GATE7_SCRIPT=str(script), TQA_STACK_RESULT=str(result))
        args = ["corepack", "pnpm", "exec", "playwright", "test", "--config", "playwright.stack.config.ts"]
        completed = subprocess.run(args, cwd=ROOT / "web", env=env, capture_output=True, text=True,
                                   timeout=PAGE_SECONDS, check=False)
        print(compose.redact(completed.stdout[-6000:], self.env), flush=True)
        if completed.returncode != 0:
            print(compose.redact(completed.stderr[-4000:], self.env), file=sys.stderr, flush=True)
            raise AssertionError("the channel page spec failed")
        return json.loads(result.read_text())

    def events(self, conversation):
        return self.request("/api/v1/conversations/{}/events?after=0&limit=1000".format(conversation))["items"]

    def until_runs_end(self):
        """Both agents' runs ended; the channel waits can be long, so this polls rather than streams."""
        deadline = time.monotonic() + STEP_SECONDS
        pending = dict(self.agents)
        while pending and time.monotonic() < deadline:
            for agent, conversation in list(pending.items()):
                kinds = [(frame.get("event") or {}).get("type") for frame in self.events(conversation)]
                if "run.completed" in kinds or "run.failed" in kinds:
                    del pending[agent]
            time.sleep(2)
        assert not pending, "these runs did not end: " + ", ".join(pending)

    def diagnose(self):
        """On a failure: the channel's log and each agent's tool calls and results, so the failing step names itself."""
        channel = getattr(self, "channel", None)
        if channel is None:
            return
        try:
            messages = self.request("/api/v1/channels/{}/messages?limit=200".format(channel))["messages"]
            print("Channel log: " + json.dumps([(item["seq"], item["body"]["type"], item["author"]["kind"],
                                                  item["author"].get("name")) for item in messages]), flush=True)
            for agent, conversation in self.agents.items():
                steps = [((frame.get("event") or {}).get("type"), json.dumps(frame.get("event"))[:400])
                         for frame in self.events(conversation)
                         if (frame.get("event") or {}).get("type") in ("tool.call", "tool.result", "run.failed",
                                                                         "run.completed")]
                print(agent + " steps: " + json.dumps(steps), flush=True)
        except Exception as error:
            print("Diagnosis unavailable: " + str(error), flush=True)

    def observe(self, page):
        """C1-C5 from the log, the proposals, the board, both runs' events and the page's result."""
        names = {conversation: agent for agent, conversation in self.agents.items()}
        base = "/api/v1/channels/" + self.channel
        observed = {}
        observed.update(evidence.check_log(self.request(base + "/messages?limit=200")["messages"], names))
        observed.update(evidence.check_decisions(self.request(base + "/proposals/2"),
                                                 self.request(base + "/proposals/3")))
        observed.update(evidence.check_board(self.request(base + "/tasks"), names))
        for agent, conversation in self.agents.items():
            observed.update(evidence.check_run(agent, self.events(conversation)))
        if page is not None:
            observed.update(evidence.check_page(page))
        return observed


def interrupted(_number, _frame):
    raise InterruptedError("E2E-CHANNEL-016 was interrupted or exceeded its deadline")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--person", choices=["api", "page"], default="api")
    args = parser.parse_args()
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-gate7-")
    directory = compose.installation(temporary)
    stack, failure = ChannelStack(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(DEADLINE_SECONDS)
    try:
        stack.prepare()
        stack.setup()
        stack.start_agents()
        page = stack.person_by_page() if args.person == "page" else stack.person_by_api()
        stack.until_runs_end()
        print("E2E-CHANNEL-016 passed: " + json.dumps(stack.observe(page)), flush=True)
    except BaseException as error:
        failure = error
        print("E2E-CHANNEL-016 failed: " + compose.redact(str(error), env), flush=True)
        stack.diagnose()
        for service in ("gateway", "fake-planner", "fake-builder"):
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
