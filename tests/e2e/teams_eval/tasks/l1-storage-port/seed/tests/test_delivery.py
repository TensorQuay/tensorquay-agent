import os
import tempfile
import unittest

from app import db
from app.delivery import create_delivery, delete_delivery, find_deliverys, get_delivery, list_deliverys, update_delivery


class DeliveryTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_delivery(self.path, 18, 'state-1', 15)
        self.assertEqual(get_delivery(self.path, new)["shipment_id"], 18)

    def test_list(self):
        create_delivery(self.path, 18, 'state-1', 15)
        create_delivery(self.path, 25, 'state-2', 22)
        self.assertEqual(len(list_deliverys(self.path)), 2)

    def test_find(self):
        create_delivery(self.path, 18, 'state-1', 15)
        create_delivery(self.path, 25, 'state-2', 22)
        self.assertEqual([row["shipment_id"] for row in find_deliverys(self.path, 25)],
                         [25])

    def test_update(self):
        new = create_delivery(self.path, 18, 'state-1', 15)
        self.assertTrue(update_delivery(self.path, new, 25, 'state-2', 22))
        self.assertEqual(get_delivery(self.path, new)["attempts"], 22)

    def test_delete(self):
        new = create_delivery(self.path, 18, 'state-1', 15)
        self.assertTrue(delete_delivery(self.path, new))
        self.assertIsNone(get_delivery(self.path, new))
        self.assertFalse(delete_delivery(self.path, new))
