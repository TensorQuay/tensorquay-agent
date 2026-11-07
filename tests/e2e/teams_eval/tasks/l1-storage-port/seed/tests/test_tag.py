import os
import tempfile
import unittest

from app import db
from app.tag import create_tag, delete_tag, find_tags, get_tag, list_tags, update_tag


class TagTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_tag(self.path, 'name-1', 'colour-1', 11)
        self.assertEqual(get_tag(self.path, new)["name"], 'name-1')

    def test_list(self):
        create_tag(self.path, 'name-1', 'colour-1', 11)
        create_tag(self.path, 'name-2', 'colour-2', 18)
        self.assertEqual(len(list_tags(self.path)), 2)

    def test_find(self):
        create_tag(self.path, 'name-1', 'colour-1', 11)
        create_tag(self.path, 'name-2', 'colour-2', 18)
        self.assertEqual([row["name"] for row in find_tags(self.path, 'name-2')],
                         ['name-2'])

    def test_update(self):
        new = create_tag(self.path, 'name-1', 'colour-1', 11)
        self.assertTrue(update_tag(self.path, new, 'name-2', 'colour-2', 18))
        self.assertEqual(get_tag(self.path, new)["uses"], 18)

    def test_delete(self):
        new = create_tag(self.path, 'name-1', 'colour-1', 11)
        self.assertTrue(delete_tag(self.path, new))
        self.assertIsNone(get_tag(self.path, new))
        self.assertFalse(delete_tag(self.path, new))
