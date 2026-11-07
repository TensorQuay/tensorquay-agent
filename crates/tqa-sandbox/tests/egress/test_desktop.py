"""SEC-057/058's fixture and SEC-001's Desktop search (T12), without Docker: the scripted turns, the page, the search."""
import unittest

from egress import desktop


class DesktopScenarioTests(unittest.TestCase):
    def test_every_computer_call_carries_the_strict_schemas_fields(self):
        fields = {"action", "ref", "text", "keys", "ms", "direction", "amount"}
        calls = [event for response in desktop.scenario("192.0.2.10")["responses"] for event in response["events"]
                 if event.get("type") == "tool_call" and event["name"] == "computer"]
        self.assertEqual([call["arguments"]["action"] for call in calls], ["key", "type", "key", "wait", "observe"])
        for call in calls:
            self.assertEqual(set(call["arguments"]), fields)

    def test_the_page_is_opened_observed_then_obeyed_in_that_order(self):
        names = [event["name"] for response in desktop.scenario("192.0.2.10")["responses"]
                 for event in response["events"] if event.get("type") == "tool_call"]
        self.assertEqual(names, ["read_file", "computer", "computer", "computer", "computer", "computer",
                                 "shell", "fetch_url", "fetch_url"])
        typed = desktop.scenario("192.0.2.10")["responses"][2]["events"][0]["arguments"]["text"]
        self.assertEqual(typed, "file://" + desktop.PAGE)

    def test_the_page_carries_the_injection_and_the_allowed_fetch_names_the_upstream(self):
        self.assertIn(desktop.INJECTED, desktop.page())
        last = desktop.scenario("192.0.2.10")["responses"][8]["events"][0]["arguments"]["url"]
        self.assertEqual(last, "http://192.0.2.10:8081/allowed")


if __name__ == "__main__":
    unittest.main()


class DesktopSecretSearchTests(unittest.TestCase):
    """SEC-001 repeated in a Desktop sandbox (ADR-0039 T12): where a secret shows is named, never the secret itself."""

    def test_a_secret_anywhere_is_named_by_its_place_and_number(self):
        places = {"the environment": "PATH=/usr/bin\nTQA_X=s3cret-two", "the screen": "Sign in", "the events": "[]"}
        self.assertEqual(desktop.leaked(["s3cret-one", "s3cret-two"], places), ["secret 2 in the environment"])

    def test_nothing_found_and_empty_secrets_are_never_matched(self):
        self.assertEqual(desktop.leaked(["s3cret-one", ""], {"the screen": "anything at all"}), [])

    def test_every_process_s_environment_is_read(self):
        command = desktop.ENVIRONMENTS
        self.assertIn("/proc/[0-9]*/environ", " ".join(command))
