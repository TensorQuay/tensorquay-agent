import os
import tempfile
import unittest

from app import db
from app.order import create_order, delete_order, find_orders, get_order, list_orders, update_order


class OrderTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_order(self.path, 18, 'status-1', 18)
        self.assertEqual(get_order(self.path, new)["customer_id"], 18)

    def test_list(self):
        create_order(self.path, 18, 'status-1', 18)
        create_order(self.path, 25, 'status-2', 25)
        self.assertEqual(len(list_orders(self.path)), 2)

    def test_find(self):
        create_order(self.path, 18, 'status-1', 18)
        create_order(self.path, 25, 'status-2', 25)
        self.assertEqual([row["customer_id"] for row in find_orders(self.path, 25)],
                         [25])

    def test_update(self):
        new = create_order(self.path, 18, 'status-1', 18)
        self.assertTrue(update_order(self.path, new, 25, 'status-2', 25))
        self.assertEqual(get_order(self.path, new)["total_cents"], 25)

    def test_delete(self):
        new = create_order(self.path, 18, 'status-1', 18)
        self.assertTrue(delete_order(self.path, new))
        self.assertIsNone(get_order(self.path, new))
        self.assertFalse(delete_order(self.path, new))
