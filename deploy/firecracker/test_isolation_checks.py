"""SEC-013 on the microVM: the scripted turns, the in-guest refusal probe and the traffic control (no KVM)."""
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guest_search  # noqa: E402
import isolation_checks as checks  # noqa: E402

CREDENTIAL = "fake-connector-token-bm90LWluLXRoZS1zY2VuYXJpbw"


class ConnectorScenario(unittest.TestCase):
    def responses(self):
        labelled = [("form:{}".format(index), form) for index, form in enumerate(guest_search.forms(CREDENTIAL))]
        return checks.connector_scenario("mcp.tracker.create_issue", {"title": "Crash"}, labelled, "elsewhere",
                                         "http://gateway:8080")

    def test_the_call_then_the_search_and_probe_then_an_answer(self):
        responses = self.responses()
        events = [response["events"][0] for response in responses]
        self.assertEqual(events[0], {"type": "tool_call", "name": "mcp.tracker.create_issue",
                                     "arguments": {"title": "Crash"}, "split_into": 1})
        self.assertEqual(events[1]["name"], "shell")
        command = events[1]["arguments"]["command"]
        self.assertIn(guest_search.MARK, command)
        self.assertTrue(command.startswith("printf '%s%s' 'hashed-control-"), command[:80])
        self.assertIn(guest_search.CONTROL_PATH, command)
        self.assertIn(checks.OUTSIDE_MARK, command)
        self.assertIn("/mcp/elsewhere", command)
        self.assertEqual(events[2]["type"], "text")

    def test_nothing_of_the_credential_is_in_the_script(self):
        text = json.dumps(self.responses())
        for start in range(len(CREDENTIAL) - 7):
            self.assertNotIn(CREDENTIAL[start:start + 8], text)

    def test_the_probe_reads_the_token_in_the_guest_and_never_prints_it(self):
        probe = checks.outside_probe("elsewhere", "http://gateway:8080")
        self.assertIn("$(cat /run/tqa/token)", probe)
        self.assertNotIn("echo $(cat /run/tqa/token)", probe)

    def test_the_probe_names_the_gateway_itself_not_through_the_runtimes_environment(self):
        # The shell tool clears the environment (PATH, LANG, TERM and an allow-list), so TQA_GATEWAY_URL is not there:
        # the candidate 6 run's probe asked a relative URL and got curl's 000.
        probe = checks.outside_probe("elsewhere", "http://gateway:8080/")
        self.assertIn('"http://gateway:8080/mcp/elsewhere"', probe)
        self.assertNotIn("TQA_GATEWAY_URL", probe)
        self.assertIn('--noproxy "*"', probe, "straight to the gateway, never through the egress proxy")


class Outside(unittest.TestCase):
    def test_the_probe_answer_is_read_from_its_marked_line(self):
        output = "noise\n{}403 {{\"code\":\"connector_not_allowed\"}}\nmore\n".format(checks.OUTSIDE_MARK)
        self.assertEqual(checks.outside_answer(output), ("403", '{"code":"connector_not_allowed"}'))
        with self.assertRaises(AssertionError):
            checks.outside_answer("no probe line\n")


class Traffic(unittest.TestCase):
    def test_the_capture_saw_the_call_with_its_authorization(self):
        call = (b"POST /mcp/tracker HTTP/1.1\r\nAuthorization: Bearer abc\r\n\r\n"
                b'{"jsonrpc":"2.0","method":"tools/call"}')
        self.assertTrue(checks.saw_the_call({("a",): b"other", ("b",): call}, "tracker"))
        self.assertTrue(checks.saw_the_call({("b",): call.replace(b"Authorization", b"authorization")}, "tracker"))

    def test_a_stream_without_the_call_or_its_authorization_is_not_enough(self):
        self.assertFalse(checks.saw_the_call({("b",): b"POST /mcp/tracker\r\n\r\ntools/call"}, "tracker"))
        self.assertFalse(checks.saw_the_call({("b",): b"POST /mcp/elsewhere\r\nAuthorization: Bearer x\r\n\r\n"
                                                        b"tools/call"}, "tracker"))
        self.assertFalse(checks.saw_the_call({}, "tracker"))


if __name__ == "__main__":
    unittest.main()
