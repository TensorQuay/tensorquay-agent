import unittest

from app import orders, reports, sync, users


class Callers(unittest.TestCase):
    def test_orders(self):
        self.assertEqual(orders.order_summary("1"), "order 1: 2 item(s), 13.70 EUR")
        self.assertEqual(orders.order_summary("9"), "order 9: not found")
        self.assertEqual(orders.place_order(["pen"]), "placed")
        self.assertEqual(orders.place_order([]), "refused (400)")

    def test_users(self):
        self.assertEqual(users.user_name("8"), "Alan Turing")
        self.assertEqual(users.user_name("1"), "unknown user")

    def test_reports(self):
        self.assertEqual(reports.daily_total("2026-10-02"), 12.25)
        self.assertEqual(reports.daily_total("2026-01-01"), 0.0)

    def test_sync(self):
        self.assertEqual(sync.push([{"id": 1}, {"id": 2}]), "synced 2")
        self.assertEqual(sync.push([]), "sync failed")
