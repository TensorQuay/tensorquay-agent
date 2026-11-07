import os
import tempfile
import unittest

from app import db
from app.address import create_address, delete_address, find_addresss, get_address, list_addresss, update_address


class AddressTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_address(self.path, 18, 'street-1', 'postcode-1')
        self.assertEqual(get_address(self.path, new)["customer_id"], 18)

    def test_list(self):
        create_address(self.path, 18, 'street-1', 'postcode-1')
        create_address(self.path, 25, 'street-2', 'postcode-2')
        self.assertEqual(len(list_addresss(self.path)), 2)

    def test_find(self):
        create_address(self.path, 18, 'street-1', 'postcode-1')
        create_address(self.path, 25, 'street-2', 'postcode-2')
        self.assertEqual([row["customer_id"] for row in find_addresss(self.path, 25)],
                         [25])

    def test_update(self):
        new = create_address(self.path, 18, 'street-1', 'postcode-1')
        self.assertTrue(update_address(self.path, new, 25, 'street-2', 'postcode-2'))
        self.assertEqual(get_address(self.path, new)["postcode"], 'postcode-2')

    def test_delete(self):
        new = create_address(self.path, 18, 'street-1', 'postcode-1')
        self.assertTrue(delete_address(self.path, new))
        self.assertIsNone(get_address(self.path, new))
        self.assertFalse(delete_address(self.path, new))
