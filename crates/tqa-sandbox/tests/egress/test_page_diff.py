"""WEB-E2E-010's page pair rule (page_diff.py), one test per condition."""
import base64
import unittest

from egress import page_diff

HEAD = '["$","$Lc","{}",{{"children":"$Ld"}}],["$","div","{}",{{}}]'


def pair(first_id, second_id, extra=""):
    return HEAD.format(first_id, first_id) + extra, HEAD.format(second_id, second_id) + extra


class PageDiffTests(unittest.TestCase):
    def test_next_head_key_passes(self):
        first, second = pair("Cwu8-7holI78ZjjO_xt-Xv", "02gDi8tEIVjvMSnM6kdZmv")
        self.assertEqual(page_diff.problems(first, second, page_diff.request_values(["text/html", "1"], [], [])), [])

    def test_identical_pages_pass(self):
        self.assertEqual(page_diff.problems("a b c", "a b c", set()), [])

    def test_a_short_differing_token_fails(self):
        first, second = pair("Cwu8-7holI78ZjjO_xt-Xv", "Cwu8-7holI78ZjjO_xt-Xv")
        reasons = page_diff.problems(first + " count 3", second + " count 4", set())
        self.assertEqual(reasons, ["short tokens differ: 3, 4"])

    def test_a_moved_or_repeated_token_counts(self):
        self.assertEqual(page_diff.differing("aaaaaaaaaaaaaaaaaaaa x", "x aaaaaaaaaaaaaaaaaaaa"), {})
        changed = page_diff.differing("aaaaaaaaaaaaaaaaaaaa", "aaaaaaaaaaaaaaaaaaaa aaaaaaaaaaaaaaaaaaaa")
        self.assertEqual(changed, {"aaaaaaaaaaaaaaaaaaaa": 1})

    def test_nine_differing_ids_fail(self):
        ids = ["{:0>20}".format(number) for number in range(9)]
        first = " ".join("A" + value for value in ids)
        second = " ".join("B" + value for value in ids)
        self.assertIn("18 tokens differ (at most 8)", page_diff.problems(first, second, set()))

    def test_a_token_equal_to_or_holding_a_cookie_value_fails_and_is_never_shown(self):
        cookie = "sessionValue-1234567890abcdef"
        encoded = base64.urlsafe_b64encode(b"decodedSessionSecret-0123456789").decode().rstrip("=")
        values = page_diff.request_values(["text/html"], [cookie, encoded], [])
        for token in (cookie, "xx" + cookie + "yy", "decodedSessionSecret-0123456789"):
            first, second = "keep " + token, "keep freshRandomId-0123456789ab"
            reasons = page_diff.problems(first, second, values)
            self.assertEqual(reasons, ["1 differing tokens match request data"], token)
            self.assertFalse(any(cookie in reason for reason in reasons))

    def test_a_task_or_run_id_fails(self):
        values = page_diff.request_values([], [], ["0190b2a4-7e1c-7d2a-9a55-3f1c2b8e4d01"])
        reasons = page_diff.problems("x 0190b2a4-7e1c-7d2a-9a55-3f1c2b8e4d01", "x someOtherRandomId-00000000", values)
        self.assertEqual(reasons, ["1 differing tokens match request data"])


if __name__ == "__main__":
    unittest.main()
