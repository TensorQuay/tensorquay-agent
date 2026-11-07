"""SBX-C-021's driver runs the web-search hint's own commands: the flags it uses are the ones the prompt gives
(PROMPT-U-021's golden), the search address is queried once, and every call is scripted for the fake model."""
import json
from pathlib import Path
import tempfile
import unittest

from egress import web_search

GOLDEN = Path(__file__).resolve().parents[3] / "tqa-prompt/tests/fixtures/system-v14-workstation-open-web.txt"


class WebSearchDriverTests(unittest.TestCase):
    # TEST: SBX-C-021
    def test_the_commands_are_the_hints(self):
        lines = GOLDEN.read_text().splitlines()
        facts = next(line for line in lines if line.startswith("Facts:"))
        hint = next(line for line in lines if line.startswith("Search:"))
        api = "https://en.wikipedia.org/w/api.php?action=query&list=search&format=json&srlimit=5&srsearch="
        self.assertIn("curl -s '" + api, facts)
        self.assertTrue(web_search.FACTS.startswith("curl -s '" + web_search.FACTS_SEARCH + "'"))
        self.assertTrue(web_search.FACTS_SEARCH.startswith(api))
        self.assertIn("curl -s 'https://html.duckduckgo.com/html/?q=", hint)
        self.assertTrue(web_search.CURL.startswith("curl -s '" + web_search.SEARCH + "'"))
        self.assertTrue(web_search.SEARCH.startswith("https://html.duckduckgo.com/html/?q="))
        # Version 14: the proxy without the URL's trailing slash, which Chromium refuses (ERR_NO_SUPPORTED_PROXIES).
        chromium = "chromium --headless --no-sandbox --proxy-server=${HTTPS_PROXY%/} --dump-dom"
        self.assertIn(chromium, hint)
        self.assertIn(chromium, web_search.CHROMIUM)
        self.assertIn("fetch_url", lines[lines.index(hint) + 1])

    # TEST: SBX-C-021
    def test_one_query_a_run(self):
        # curl once; Chromium on the search page only when curl found no results, else on the other allowed page.
        self.assertEqual(web_search.FACTS.count(web_search.FACTS_SEARCH), 1)
        self.assertEqual(web_search.CURL.count(web_search.SEARCH), 1)
        self.assertIn("if grep -q 'result__a' search.html; then url='" + web_search.PAGE + "'", web_search.CHROMIUM)
        self.assertEqual(web_search.CHROMIUM.count("chromium --headless"), 1)

    # TEST: SBX-C-021
    def test_the_scenario_scripts_the_five_calls_and_allows_only_three_public_hosts(self):
        stack = web_search.SearchStack.__new__(web_search.SearchStack)
        with tempfile.TemporaryDirectory() as directory:
            stack.directory = Path(directory)
            stack.scenario()
            scenario = json.loads((Path(directory) / "scenario.json").read_text())
        calls = [response["events"][0] for response in scenario["responses"][:5]]
        self.assertEqual([(call["name"], call["arguments"]) for call in calls], [
            ("shell", {"command": web_search.FACTS}), ("shell", {"command": web_search.CURL}), ("shell", {"command": web_search.CHROMIUM}),
            ("fetch_url", {"url": web_search.PAGE}), ("fetch_url", {"url": web_search.DENIED})])
        self.assertEqual(dict(web_search.SearchStack.extra_allow),
                         {"en.wikipedia.org": [443], "html.duckduckgo.com": [443], "example.com": [443]})
        self.assertNotIn("denied.invalid", json.dumps(web_search.SearchStack.extra_allow))


    # TEST: SBX-C-021
    def test_chromiums_reason_leaves_out_the_bus_noise(self):
        err = "\n".join(["[1:2:ERROR:dbus/bus.cc:405] Failed to connect to the bus"] * 40
                         + ["[1:2:ERROR:net] net::ERR_TUNNEL_CONNECTION_FAILED"])
        self.assertEqual(web_search.chromium_reason(err), "[1:2:ERROR:net] net::ERR_TUNNEL_CONNECTION_FAILED")
        self.assertEqual(web_search.chromium_reason("[x] dbus only"), "(nothing but D-Bus noise)")

    # TEST: SBX-C-021
    def test_a_failure_keeps_what_was_seen_with_its_verdict(self):
        evidence = {"events": [{"seq": 1}], "results": [{"status": "ok", "content": "http 200"}],
                    "search.html": "<html>", "dom.html": "", "chromium.err": "dbus", "search_status": "200"}
        with tempfile.TemporaryDirectory() as directory:
            web_search.keep(evidence, Path(directory), error="Chromium dumped no page")
            summary = json.loads((Path(directory) / "summary.json").read_text())
            kept = sorted(path.name for path in Path(directory).iterdir())
        self.assertEqual(summary, {"facts_status": None, "facts_bytes": None, "facts_results": None,
                                   "facts_titles": None, "search_status": "200",
                                   "search_page": None, "chromium_target": None, "verdict": "failed",
                                   "error": "Chromium dumped no page"})
        self.assertEqual(kept, ["chromium.err", "dom.html", "events.json", "results.json", "search.html",
                                "summary.json"])


if __name__ == "__main__":
    unittest.main()
