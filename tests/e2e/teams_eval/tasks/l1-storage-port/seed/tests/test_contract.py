import os
import tempfile
import unittest

from app import db
from app.contract import create_contract, delete_contract, find_contracts, get_contract, list_contracts, update_contract


class ContractTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_contract(self.path, 18, 'starts-1', 'ends-1')
        self.assertEqual(get_contract(self.path, new)["supplier_id"], 18)

    def test_list(self):
        create_contract(self.path, 18, 'starts-1', 'ends-1')
        create_contract(self.path, 25, 'starts-2', 'ends-2')
        self.assertEqual(len(list_contracts(self.path)), 2)

    def test_find(self):
        create_contract(self.path, 18, 'starts-1', 'ends-1')
        create_contract(self.path, 25, 'starts-2', 'ends-2')
        self.assertEqual([row["supplier_id"] for row in find_contracts(self.path, 25)],
                         [25])

    def test_update(self):
        new = create_contract(self.path, 18, 'starts-1', 'ends-1')
        self.assertTrue(update_contract(self.path, new, 25, 'starts-2', 'ends-2'))
        self.assertEqual(get_contract(self.path, new)["ends"], 'ends-2')

    def test_delete(self):
        new = create_contract(self.path, 18, 'starts-1', 'ends-1')
        self.assertTrue(delete_contract(self.path, new))
        self.assertIsNone(get_contract(self.path, new))
        self.assertFalse(delete_contract(self.path, new))
