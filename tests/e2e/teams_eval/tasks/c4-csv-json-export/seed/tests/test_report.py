import unittest

from report import render_text, totals


class ReportTest(unittest.TestCase):
    def test_totals_sorted_by_region(self):
        rows = [{"region": "b", "amount": 2}, {"region": "a", "amount": 1}, {"region": "b", "amount": 3}]
        self.assertEqual(totals(rows), [("a", 1), ("b", 5)])

    def test_text(self):
        self.assertEqual(render_text([("a", 1), ("b", 5)]), "a: 1\nb: 5")
