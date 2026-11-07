import os
import tempfile
import unittest

from app import db
from app.warehouse import create_warehouse, delete_warehouse, find_warehouses, get_warehouse, list_warehouses, update_warehouse


class WarehouseTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_warehouse(self.path, 'code-1', 'city-1', 15)
        self.assertEqual(get_warehouse(self.path, new)["code"], 'code-1')

    def test_list(self):
        create_warehouse(self.path, 'code-1', 'city-1', 15)
        create_warehouse(self.path, 'code-2', 'city-2', 22)
        self.assertEqual(len(list_warehouses(self.path)), 2)

    def test_find(self):
        create_warehouse(self.path, 'code-1', 'city-1', 15)
        create_warehouse(self.path, 'code-2', 'city-2', 22)
        self.assertEqual([row["code"] for row in find_warehouses(self.path, 'code-2')],
                         ['code-2'])

    def test_update(self):
        new = create_warehouse(self.path, 'code-1', 'city-1', 15)
        self.assertTrue(update_warehouse(self.path, new, 'code-2', 'city-2', 22))
        self.assertEqual(get_warehouse(self.path, new)["capacity"], 22)

    def test_delete(self):
        new = create_warehouse(self.path, 'code-1', 'city-1', 15)
        self.assertTrue(delete_warehouse(self.path, new))
        self.assertIsNone(get_warehouse(self.path, new))
        self.assertFalse(delete_warehouse(self.path, new))
