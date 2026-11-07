import os
import tempfile
import unittest

from app import db
from app.category import create_category, delete_category, find_categorys, get_category, list_categorys, update_category


class CategoryTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_category(self.path, 'name-1', 16, 15)
        self.assertEqual(get_category(self.path, new)["name"], 'name-1')

    def test_list(self):
        create_category(self.path, 'name-1', 16, 15)
        create_category(self.path, 'name-2', 23, 22)
        self.assertEqual(len(list_categorys(self.path)), 2)

    def test_find(self):
        create_category(self.path, 'name-1', 16, 15)
        create_category(self.path, 'name-2', 23, 22)
        self.assertEqual([row["name"] for row in find_categorys(self.path, 'name-2')],
                         ['name-2'])

    def test_update(self):
        new = create_category(self.path, 'name-1', 16, 15)
        self.assertTrue(update_category(self.path, new, 'name-2', 23, 22))
        self.assertEqual(get_category(self.path, new)["position"], 22)

    def test_delete(self):
        new = create_category(self.path, 'name-1', 16, 15)
        self.assertTrue(delete_category(self.path, new))
        self.assertIsNone(get_category(self.path, new))
        self.assertFalse(delete_category(self.path, new))
