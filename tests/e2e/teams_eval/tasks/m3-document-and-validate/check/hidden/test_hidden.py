import unittest

from svc import app


class HeldOut(unittest.TestCase):
    def setUp(self):
        app.ITEMS.clear()

    def test_invalid_items_are_refused(self):
        for body in ({}, {"name": "", "price": 1}, {"name": "x" * 81, "price": 1}, {"name": "pen", "price": -1},
                     {"name": "pen", "price": "1"}, {"name": 5, "price": 1}):
            self.assertEqual(app.dispatch("POST", "/items", body)[0], 400, body)
        self.assertEqual(app.ITEMS, {})

    def test_valid_items_still_work(self):
        status, item = app.dispatch("POST", "/items", {"name": "pen", "price": 1.5})
        self.assertEqual((status, item["name"]), (201, "pen"))
        self.assertEqual(app.dispatch("PUT", "/items/1", {"price": -2})[0], 400)
        self.assertEqual(app.dispatch("PUT", "/items/1", {"price": 2})[1]["price"], 2)
        self.assertEqual(app.dispatch("GET", "/items/1")[0], 200)
        self.assertEqual(app.dispatch("DELETE", "/items/1")[0], 204)
        self.assertEqual(app.dispatch("GET", "/items/9")[0], 404)
