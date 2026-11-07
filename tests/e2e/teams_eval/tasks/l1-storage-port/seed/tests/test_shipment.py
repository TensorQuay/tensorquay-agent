import os
import tempfile
import unittest

from app import db
from app.shipment import create_shipment, delete_shipment, find_shipments, get_shipment, list_shipments, update_shipment


class ShipmentTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_shipment(self.path, 15, 'carrier-1', 'tracking-1')
        self.assertEqual(get_shipment(self.path, new)["order_id"], 15)

    def test_list(self):
        create_shipment(self.path, 15, 'carrier-1', 'tracking-1')
        create_shipment(self.path, 22, 'carrier-2', 'tracking-2')
        self.assertEqual(len(list_shipments(self.path)), 2)

    def test_find(self):
        create_shipment(self.path, 15, 'carrier-1', 'tracking-1')
        create_shipment(self.path, 22, 'carrier-2', 'tracking-2')
        self.assertEqual([row["order_id"] for row in find_shipments(self.path, 22)],
                         [22])

    def test_update(self):
        new = create_shipment(self.path, 15, 'carrier-1', 'tracking-1')
        self.assertTrue(update_shipment(self.path, new, 22, 'carrier-2', 'tracking-2'))
        self.assertEqual(get_shipment(self.path, new)["tracking"], 'tracking-2')

    def test_delete(self):
        new = create_shipment(self.path, 15, 'carrier-1', 'tracking-1')
        self.assertTrue(delete_shipment(self.path, new))
        self.assertIsNone(get_shipment(self.path, new))
        self.assertFalse(delete_shipment(self.path, new))
