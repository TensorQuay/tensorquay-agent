import unittest

from shop.cart import Cart
from shop.pricing import calc_total


class PricingTest(unittest.TestCase):
    def test_total_with_tax(self):
        self.assertEqual(calc_total([(1000, 2)], 10), 2200)

    def test_cart(self):
        cart = Cart(10)
        cart.add(500, 3)
        self.assertEqual(cart.total(), 1650)
