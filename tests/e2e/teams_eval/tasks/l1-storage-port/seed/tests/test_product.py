import os
import tempfile
import unittest

from app import db
from app.product import create_product, delete_product, find_products, get_product, list_products, update_product


class ProductTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_product(self.path, 'sku-1', 'title-1', 18)
        self.assertEqual(get_product(self.path, new)["sku"], 'sku-1')

    def test_list(self):
        create_product(self.path, 'sku-1', 'title-1', 18)
        create_product(self.path, 'sku-2', 'title-2', 25)
        self.assertEqual(len(list_products(self.path)), 2)

    def test_find(self):
        create_product(self.path, 'sku-1', 'title-1', 18)
        create_product(self.path, 'sku-2', 'title-2', 25)
        self.assertEqual([row["sku"] for row in find_products(self.path, 'sku-2')],
                         ['sku-2'])

    def test_update(self):
        new = create_product(self.path, 'sku-1', 'title-1', 18)
        self.assertTrue(update_product(self.path, new, 'sku-2', 'title-2', 25))
        self.assertEqual(get_product(self.path, new)["price_cents"], 25)

    def test_delete(self):
        new = create_product(self.path, 'sku-1', 'title-1', 18)
        self.assertTrue(delete_product(self.path, new))
        self.assertIsNone(get_product(self.path, new))
        self.assertFalse(delete_product(self.path, new))
