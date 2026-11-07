import os
import tempfile
import unittest

from app import db
from app.customer import create_customer, delete_customer, find_customers, get_customer, list_customers, update_customer


class CustomerTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_customer(self.path, 'name-1', 'email-1', 'city-1')
        self.assertEqual(get_customer(self.path, new)["name"], 'name-1')

    def test_list(self):
        create_customer(self.path, 'name-1', 'email-1', 'city-1')
        create_customer(self.path, 'name-2', 'email-2', 'city-2')
        self.assertEqual(len(list_customers(self.path)), 2)

    def test_find(self):
        create_customer(self.path, 'name-1', 'email-1', 'city-1')
        create_customer(self.path, 'name-2', 'email-2', 'city-2')
        self.assertEqual([row["name"] for row in find_customers(self.path, 'name-2')],
                         ['name-2'])

    def test_update(self):
        new = create_customer(self.path, 'name-1', 'email-1', 'city-1')
        self.assertTrue(update_customer(self.path, new, 'name-2', 'email-2', 'city-2'))
        self.assertEqual(get_customer(self.path, new)["city"], 'city-2')

    def test_delete(self):
        new = create_customer(self.path, 'name-1', 'email-1', 'city-1')
        self.assertTrue(delete_customer(self.path, new))
        self.assertIsNone(get_customer(self.path, new))
        self.assertFalse(delete_customer(self.path, new))
