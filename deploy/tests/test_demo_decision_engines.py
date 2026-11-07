"""The settings suite's decision engines (demo_decision_engines.py, WEB-E2E-070): two file-only http engines at the
fake model, valid TOML, appended last."""

import tomllib
import unittest

import demo_decision_engines


class Engines(unittest.TestCase):
    def test_two_insecure_http_engines_at_the_fake_model(self):
        document = tomllib.loads("listen = '0.0.0.0:8080'\n[sandbox]\nmax_live = 4\n"
                                 + demo_decision_engines.engines_toml())
        self.assertEqual(document["sandbox"], {"max_live": 4}, "nothing before them is captured")
        engines = {engine["name"]: engine for engine in document["decision_engines"]}
        self.assertEqual(set(engines), {"stopper", "passer"})
        for name, engine in engines.items():
            self.assertEqual(engine["url"], "http://fake-model:8080/engines/" + name)
            self.assertIs(engine["insecure_http"], True)


if __name__ == "__main__":
    unittest.main()
