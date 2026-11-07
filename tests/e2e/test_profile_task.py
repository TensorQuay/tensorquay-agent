"""E2E-PROFILE-007: the profile task's connector server, configuration, replies and checks, without a stack.

Run: python3 -m unittest discover -s tests/e2e -p 'test_*.py' -v
"""

import json
import socket
import subprocess
import sys
import time
import unittest
import urllib.request

import profile_task_evidence as evidence


def event(kind, **fields):
    return {"type": "event", "event": dict(fields, type=kind)}


def journal(**changes):
    """A journal the checks accept, with one part replaced."""
    parts = {
        "servers": [event("mcp.server", name="tracker", state="ready", tools=1)],
        "calls": [event("tool.call", name="shell"), event("tool.call", name="mcp.tracker.lookup")],
        "results": [event("tool.result", status="error", content=evidence.REFUSED_SHELL),
                    event("tool.result", status="ok", content=evidence.MARKED_ANSWER + "\n")],
        "end": [event("run.completed")],
    }
    parts.update(changes)
    return ([event("run.started")] + parts["servers"] + parts["calls"] + parts["results"] + [{"type": "notice"}]
            + parts["end"])


CONVERSATION = {"profile": {"name": evidence.PROFILE_NAME}, "mcp_servers": [], "connectors": ["tracker"]}


class VerifyTests(unittest.TestCase):
    def test_a_run_using_only_the_allowed_tools_passes(self):
        self.assertEqual(evidence.verify(CONVERSATION, journal()),
                         {"calls": ["shell", "mcp.tracker.lookup"], "results": ["error", "ok"], "servers": ["tracker"]})

    def test_a_conversation_without_the_profiles_sets_fails(self):
        without_profile = {key: value for key, value in CONVERSATION.items() if key != "profile"}
        for conversation in (without_profile,
                             {**CONVERSATION, "mcp_servers": ["words"]},
                             {**CONVERSATION, "connectors": ["tracker", "wiki"]}):
            with self.assertRaises(AssertionError):
                evidence.verify(conversation, journal())

    def test_the_catalogue_server_the_profile_leaves_out_fails_the_run(self):
        servers = [event("mcp.server", name="words", state="ready", tools=1),
                   event("mcp.server", name="tracker", state="ready", tools=1)]
        with self.assertRaisesRegex(AssertionError, "words"):
            evidence.verify(CONVERSATION, journal(servers=servers))

    def test_a_disallowed_tool_that_ran_fails(self):
        results = [event("tool.result", status="ok", content="README.md"),
                   event("tool.result", status="ok", content=evidence.MARKED_ANSWER)]
        with self.assertRaises(AssertionError):
            evidence.verify(CONVERSATION, journal(results=results))

    def test_a_refusal_naming_more_tools_than_the_profile_allows_fails(self):
        wider = "unsupported: unknown tool; known tools: fetch_url, mcp.tracker.lookup, read_file"
        results = [event("tool.result", status="error", content=wider),
                   event("tool.result", status="ok", content=evidence.MARKED_ANSWER)]
        with self.assertRaises(AssertionError):
            evidence.verify(CONVERSATION, journal(results=results))

    def test_a_connector_answer_without_its_marker_fails(self):
        results = [event("tool.result", status="error", content=evidence.REFUSED_SHELL),
                   event("tool.result", status="ok", content=evidence.ANSWER)]
        with self.assertRaises(AssertionError):
            evidence.verify(CONVERSATION, journal(results=results))

    def test_a_run_that_did_not_complete_fails(self):
        with self.assertRaises(AssertionError):
            evidence.verify(CONVERSATION, journal(end=[event("run.failed")]))


class FixtureTests(unittest.TestCase):
    def test_the_profile_allows_one_builtin_and_one_connector(self):
        profile = evidence.profile()
        self.assertEqual((profile["tools"], profile["mcp_servers"], profile["connectors"]), (["read_file"], [], ["tracker"]))

    def test_the_replies_try_a_disallowed_tool_then_the_connector(self):
        names = [response["events"][0].get("name", "text") for response in evidence.replies()["responses"]]
        self.assertEqual(names, ["shell", "mcp__tracker__lookup", "text"])

    def test_the_configuration_names_the_connector_by_address(self):
        config = evidence.gateway_config("192.0.2.10", "print(1)")
        self.assertIn("url='http://192.0.2.10:8080/mcp'\ninsecure_http=true\nenabled_by_default=true", config)
        self.assertNotIn("class", config)  # the connector class was removed (ADR-0035)
        self.assertIn("[[mcp.servers]]\nname='words'", config)
        for refused in ("tracker.example", "::1", "192.0.2.10'\nx='y"):
            with self.assertRaises(ValueError):
                evidence.gateway_config(refused, "print(1)")


class ConnectorTests(unittest.TestCase):
    """The connector's MCP server, run here as the stack runs it."""

    def setUp(self):
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            self.port = reservation.getsockname()[1]
        self.server = subprocess.Popen([sys.executable, "-c", evidence.CONNECTOR, str(self.port)],
                                       stdout=subprocess.PIPE, text=True)
        self.addCleanup(self.stop)
        self.assertEqual(self.server.stdout.readline().strip(), "listening")

    def stop(self):
        self.server.kill()
        self.server.wait(timeout=10)
        self.server.stdout.close()

    def post(self, body):
        request = urllib.request.Request("http://127.0.0.1:{}/mcp".format(self.port), data=json.dumps(body).encode(),
                                         headers={"Content-Type": "application/json"})
        for _ in range(50):
            try:
                with urllib.request.urlopen(request, timeout=5) as response:
                    data = response.read()
                    return response.status, json.loads(data) if data else None
            except ConnectionRefusedError:
                time.sleep(0.1)
        raise TimeoutError("the connector did not answer")

    def test_it_answers_initialize_list_and_call(self):
        status, initialized = self.post({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})
        self.assertEqual((status, initialized["result"]["serverInfo"]["name"]), (200, "tracker"))
        self.assertEqual(self.post({"jsonrpc": "2.0", "method": "notifications/initialized"}), (202, None))
        _, listed = self.post({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
        self.assertEqual([tool["name"] for tool in listed["result"]["tools"]], ["lookup"])
        _, called = self.post({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                               "params": {"name": "lookup", "arguments": {"id": evidence.ISSUE}}})
        self.assertEqual(called, {"jsonrpc": "2.0", "id": 3,
                                  "result": {"content": [{"type": "text", "text": evidence.ANSWER}]}})


if __name__ == "__main__":
    unittest.main()
