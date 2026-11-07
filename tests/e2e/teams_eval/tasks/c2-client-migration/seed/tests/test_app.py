import unittest

from app import orders, users


class AppTest(unittest.TestCase):
    def test_summary(self):
        self.assertEqual(orders.order_summary("2"),
                         "order 2: 1 item(s), 24.00 EUR")

    def test_user(self):
        self.assertEqual(users.user_name("7"), "Ada Lovelace")
