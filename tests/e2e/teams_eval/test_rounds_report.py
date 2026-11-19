"""rounds_report's reading of the lead's round-2 starts (E2E-AGENTS-041): what the model asked for and what the gateway
answered, so a bring-back answered with a new helper is the gateway's failure, never the model's."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import rounds_report  # noqa: E402


def call(call_id, role, files=None):
    arguments = {"role": role, "workspace": "own_copy"}
    if files:
        arguments["files"] = files
    return ("2026-11-19T07:27:20Z", "tool.call", {"call_id": call_id, "name": "start_member", "arguments": arguments})


def result(call_id, content, status="ok"):
    return ("2026-11-19T07:27:21Z", "tool.result", {"call_id": call_id, "content": content, "status": status})


FOLLOW_UP = ("2026-11-19T07:27:00Z", "message.user", {"text": "Bring back totals and checker; add shares."})
ROUND_ONE = [call("a", "totals"), result("a", "Started helper totals (own copy)."), call("b", "checker"),
             result("b", "Started helper checker (own copy).")]


class BringBacks(unittest.TestCase):
    def test_what_the_model_asked_and_what_came_back(self):
        lead = ROUND_ONE + [FOLLOW_UP, call("c", "totals", "keep"), result("c", "Started helper totals-2 (own copy)."),
                            call("d", "checker", "keep"), result("d", "Brought back checker for round 2."),
                            call("e", "shares"), result("e", "Started helper shares (own copy).")]
        asked = rounds_report.bring_backs(lead, FOLLOW_UP[2]["text"])
        self.assertEqual(asked["back"], ["checker", "totals"], "the model asked for both back")
        self.assertEqual(asked["new"], ["shares"])
        self.assertEqual(asked["not_brought_back"], ["totals"], "the gateway made totals-2 instead")

    def test_every_bring_back_answered_as_one(self):
        lead = ROUND_ONE + [FOLLOW_UP, call("c", "Totals", "keep"), result("c", "Brought back totals for round 2."),
                            call("d", "checker", "fresh_copy"), result("d", "Brought back checker for round 2.")]
        asked = rounds_report.bring_backs(lead, FOLLOW_UP[2]["text"])
        self.assertEqual((asked["back"], asked["new"], asked["not_brought_back"]), (["checker", "totals"], [], []))

    def test_round_one_starts_are_not_counted(self):
        asked = rounds_report.bring_backs(ROUND_ONE, FOLLOW_UP[2]["text"])
        self.assertEqual((asked["back"], asked["new"]), ([], []))


if __name__ == "__main__":
    unittest.main()
