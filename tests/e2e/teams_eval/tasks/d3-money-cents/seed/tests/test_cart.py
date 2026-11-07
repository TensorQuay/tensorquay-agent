import unittest
from shop.cart import Cart
from shop.money import Money


class CartTests(unittest.TestCase):
    def test_gross_of_many_small_lines(self):
        cart = Cart()
        for _ in range(10):
            cart.add(Money("0.10"), 3)
        self.assertEqual(str(cart.net()), "3.00 EUR")

    def test_tax_and_gross(self):
        cart = Cart()
        cart.add(Money("19.99"), 3)
        cart.add(Money("2.49"), 1, "reduced")
        self.assertEqual(str(cart.tax()), "11.56 EUR")
        self.assertEqual(str(cart.gross()), "74.02 EUR")
