import os
import tempfile
import unittest

from app import db
from app.stock import create_stock, delete_stock, find_stocks, get_stock, list_stocks, update_stock


class StockTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_stock(self.path, 17, 19, 15)
        self.assertEqual(get_stock(self.path, new)["product_id"], 17)

    def test_list(self):
        create_stock(self.path, 17, 19, 15)
        create_stock(self.path, 24, 26, 22)
        self.assertEqual(len(list_stocks(self.path)), 2)

    def test_find(self):
        create_stock(self.path, 17, 19, 15)
        create_stock(self.path, 24, 26, 22)
        self.assertEqual([row["product_id"] for row in find_stocks(self.path, 24)],
                         [24])

    def test_update(self):
        new = create_stock(self.path, 17, 19, 15)
        self.assertTrue(update_stock(self.path, new, 24, 26, 22))
        self.assertEqual(get_stock(self.path, new)["quantity"], 22)

    def test_delete(self):
        new = create_stock(self.path, 17, 19, 15)
        self.assertTrue(delete_stock(self.path, new))
        self.assertIsNone(get_stock(self.path, new))
        self.assertFalse(delete_stock(self.path, new))
