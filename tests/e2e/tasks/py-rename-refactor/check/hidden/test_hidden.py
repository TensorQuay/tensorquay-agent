import unittest

from shop.cart import Cart
from shop.discount import discounted_total
from shop.invoice import invoice
from shop.pricing import compute_total
from shop.report import daily_total


class HeldOutTest(unittest.TestCase):
    def test_compute_total(self):
        self.assertEqual(compute_total([(1000, 2), (250, 4)], 8), 3240)
        self.assertEqual(compute_total([], 20), 0)
        self.assertEqual(compute_total([(1, 1)], 50), 2)

    def test_callers(self):
        cart = Cart(8)
        cart.add(1000, 2)
        self.assertEqual(cart.total(), 2160)
        self.assertEqual(daily_total([[(1000, 1)], [(500, 2)]], 10), 2200)
        self.assertEqual(discounted_total([(1000, 1)], 10, 20), 880)
        self.assertTrue(invoice([("x", 100, 1)], 0).endswith("Total with 0% tax: 1.00"))
