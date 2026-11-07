import os
import tempfile
import unittest

from app import db
from app.supplier import create_supplier, delete_supplier, find_suppliers, get_supplier, list_suppliers, update_supplier


class SupplierTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_supplier(self.path, 'name-1', 'country-1', 13)
        self.assertEqual(get_supplier(self.path, new)["name"], 'name-1')

    def test_list(self):
        create_supplier(self.path, 'name-1', 'country-1', 13)
        create_supplier(self.path, 'name-2', 'country-2', 20)
        self.assertEqual(len(list_suppliers(self.path)), 2)

    def test_find(self):
        create_supplier(self.path, 'name-1', 'country-1', 13)
        create_supplier(self.path, 'name-2', 'country-2', 20)
        self.assertEqual([row["name"] for row in find_suppliers(self.path, 'name-2')],
                         ['name-2'])

    def test_update(self):
        new = create_supplier(self.path, 'name-1', 'country-1', 13)
        self.assertTrue(update_supplier(self.path, new, 'name-2', 'country-2', 20))
        self.assertEqual(get_supplier(self.path, new)["rating"], 20)

    def test_delete(self):
        new = create_supplier(self.path, 'name-1', 'country-1', 13)
        self.assertTrue(delete_supplier(self.path, new))
        self.assertIsNone(get_supplier(self.path, new))
        self.assertFalse(delete_supplier(self.path, new))
