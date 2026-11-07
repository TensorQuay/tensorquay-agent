import unittest

import json

from report import render_csv, render_json, render_text, totals


class ReportTest(unittest.TestCase):
    def test_totals_sorted_by_region(self):
        rows = [{"region": "b", "amount": 2}, {"region": "a", "amount": 1}, {"region": "b", "amount": 3}]
        self.assertEqual(totals(rows), [("a", 1), ("b", 5)])

    def test_text(self):
        self.assertEqual(render_text([("a", 1), ("b", 5)]), "a: 1\nb: 5")

    def test_csv(self):
        self.assertEqual(render_csv([("a", 1)]), "region,total\na,1")

    def test_json(self):
        self.assertEqual(json.loads(render_json([("a", 1)])), [{"region": "a", "sum": 1}])
