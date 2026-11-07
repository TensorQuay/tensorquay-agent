"""TK-U-017: the live mode's private route file, route configuration and output guard. Synthetic values only.

Run: python3 -m unittest discover -s tests/e2e/live -p 'test_*.py' -v
"""
from pathlib import Path
import tempfile
import tomllib
import unittest

import live

SUITE = [
    {"name": "p1-chat", "protocol": "openai_chat", "settings": ""},
    {"name": "p2-chat-basic", "protocol": "openai_chat", "settings": "schema_keywords='basic'\n"},
]
VALUES = {"FIRST_KEY": "synthetic-live-value-one-7f3a", "SECOND_KEY": "synthetic-live-value-two-91c2",
          "PROXY_URL": "https://proxy.invalid/v1/"}
ROUTES = """
[[routes]]
name = "p1-chat"
model = "model-one"
base_url_env = "PROXY_URL"
key_env = "FIRST_KEY"

[[routes]]
name = "p2-chat-basic"
model = "model-two"
base_url = "https://compat.invalid/v1"
key_env = "SECOND_KEY"
auth_header = "x-goog-api-key"
"""


def document(text=ROUTES):
    return tomllib.loads(text)


class LiveTest(unittest.TestCase):
    def test_tk_u_017_the_routes_file_must_be_private(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "routes.toml"
            path.write_text(ROUTES)
            path.chmod(0o644)
            with self.assertRaisesRegex(live.LiveError, "mode 0600"):
                live.private_routes(path)
            path.chmod(0o600)
            self.assertEqual(live.private_routes(path), document())

    def test_tk_u_017_each_route_is_keyed_on_its_own_seeded_secret(self):
        routes = live.resolve(document(), SUITE, VALUES)
        text = live.models_toml(routes, 131072)
        parsed = tomllib.loads(text)["routes"]
        self.assertEqual([route["key_ref"] for route in parsed], ["secrets/live-p1-chat", "secrets/live-p2-chat-basic"])
        self.assertEqual(parsed[0]["base_url"], "https://proxy.invalid/v1")
        self.assertEqual(parsed[0]["auth"], {"kind": "bearer"})
        self.assertEqual(parsed[1]["auth"], {"kind": "header", "name": "x-goog-api-key"})
        self.assertEqual(parsed[1]["schema_keywords"], "basic")
        for value in (VALUES["FIRST_KEY"], VALUES["SECOND_KEY"]):
            self.assertNotIn(value, text)

    def test_tk_u_017_a_bad_routes_file_is_refused_without_a_value(self):
        cases = [
            (ROUTES.replace('name = "p2-chat-basic"', 'name = "p9"'), "a route the suite does not have"),
            (ROUTES.split("[[routes]]")[0] + "[[routes]]" + ROUTES.split("[[routes]]")[1], "no entry for p2-chat-basic"),
            (ROUTES.replace('key_env = "FIRST_KEY"', 'key_env = "first"'), "needs a model and a key_env"),
            (ROUTES.replace('base_url = "https://compat.invalid/v1"', 'base_url = "http://compat.invalid/v1"'),
             "must be an https URL"),
            (ROUTES.replace('base_url_env = "PROXY_URL"', 'base_url_env = "ABSENT"'), "must be an https URL"),
            (ROUTES.replace('model = "model-one"', 'model = "model-one"\nprice = 3'), "has a field other than"),
            (ROUTES + '\n[[routes]]\nname = "p1-chat"\n', "names p1-chat twice"),
        ]
        for text, reason in cases:
            with self.subTest(reason):
                with self.assertRaisesRegex(live.LiveError, reason) as raised:
                    live.resolve(document(text), SUITE, VALUES)
                for value in VALUES.values():
                    self.assertNotIn(value, str(raised.exception))

    def test_tk_u_017_the_guard_refuses_output_with_a_key_or_a_base_url(self):
        guard = live.Guard(live.resolve(document(), SUITE, VALUES), VALUES)
        self.assertEqual(guard.check("p1-chat probe: verified"), "p1-chat probe: verified")
        for leak in (VALUES["FIRST_KEY"], VALUES["SECOND_KEY"], "https://proxy.invalid/v1", "https://compat.invalid/v1"):
            with self.assertRaisesRegex(live.LiveError, "nothing was shown") as raised:
                guard.check("error: " + leak)
            self.assertNotIn(leak, str(raised.exception))

    def test_tk_u_017_the_summary_names_no_model_or_url(self):
        routes = live.resolve(document(), SUITE, VALUES)
        rows = [{"route": route["name"], "protocol": route["protocol"], "settings": live.settings_label(route),
                 "probe": "verified", "task": "pass", "prompt": 100, "completion": 20, "requests": 3}
                for route in routes]
        text = live.summary(rows)
        self.assertIn("| p2-chat-basic | openai_chat | schema_keywords='basic' | verified | pass | 100 / 20 | 3 |", text)
        for private in ("model-one", "model-two", "invalid"):
            self.assertNotIn(private, text)

    def test_tk_u_017_attempt_totals_read_the_flattened_tally(self):
        # The shape tqa-e2e writes: Tally is flattened into the attempt record.
        attempts = [{"task_id": "smoke-fix", "attempt": 1, "steps": 4, "prompt_tokens": 1200, "completion_tokens": 90,
                     "compactions": 0, "provider_retries": 0, "class": "pass"}]
        self.assertEqual(live.attempt_totals(attempts), (1200, 90, 4))
        self.assertEqual(live.attempt_totals([]), (0, 0, 0))

    def test_tk_u_017_timeline_lines_name_the_time_and_project(self):
        import datetime
        at = datetime.datetime(2026, 9, 27, 18, 18, 11, 123456)
        self.assertEqual(live.stamp("up", "tqa-compose-bdf55b80bf", at), "18:18:11.123 up tqa-compose-bdf55b80bf")
        self.assertEqual(live.stamp("prepare", None, at), "18:18:11.123 prepare -")

    def test_tk_u_017_attempt_details_are_kept_bounded(self):
        attempts = [{"class": "harness.provider", "detail": "provider refused: " + "x" * 2000},
                    {"class": "pass"}, {"class": "harness.provider", "detail": "second"}]
        details = live.attempt_details(attempts)
        self.assertEqual(len(details), 2)
        self.assertTrue(details[0].startswith("harness.provider: provider refused: ") and len(details[0]) <= 520)
        self.assertEqual(details[1], "harness.provider: second")

    def test_tk_u_017_the_seed_is_removed_by_a_networkless_helper(self):
        command = live.seed_removal("/tmp/tqa-live-x/installation", "toybox@sha256:abc")
        self.assertEqual(command, ["docker", "run", "--rm", "--pull", "never", "--network", "none", "--mount",
                                   "type=bind,source=/tmp/tqa-live-x/installation,target=/install",
                                   "toybox@sha256:abc", "rm", "-f", "/install/secrets.toml"])

    def test_tk_u_017_the_seeds_absence_is_checked_from_the_same_helper(self):
        command = live.seed_absent("/i", "toybox@sha256:abc")
        self.assertEqual(command[-5:], ["toybox@sha256:abc", "test", "!", "-e", "/install/secrets.toml"])
        self.assertIn("none", command)


if __name__ == "__main__":
    unittest.main()
