import os
import tempfile
import unittest

from app import db
from app.note import create_note, delete_note, find_notes, get_note, list_notes, update_note


class NoteTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_note(self.path, 'subject-1', 'body-1', 16)
        self.assertEqual(get_note(self.path, new)["subject"], 'subject-1')

    def test_list(self):
        create_note(self.path, 'subject-1', 'body-1', 16)
        create_note(self.path, 'subject-2', 'body-2', 23)
        self.assertEqual(len(list_notes(self.path)), 2)

    def test_find(self):
        create_note(self.path, 'subject-1', 'body-1', 16)
        create_note(self.path, 'subject-2', 'body-2', 23)
        self.assertEqual([row["subject"] for row in find_notes(self.path, 'subject-2')],
                         ['subject-2'])

    def test_update(self):
        new = create_note(self.path, 'subject-1', 'body-1', 16)
        self.assertTrue(update_note(self.path, new, 'subject-2', 'body-2', 23))
        self.assertEqual(get_note(self.path, new)["author_id"], 23)

    def test_delete(self):
        new = create_note(self.path, 'subject-1', 'body-1', 16)
        self.assertTrue(delete_note(self.path, new))
        self.assertIsNone(get_note(self.path, new))
        self.assertFalse(delete_note(self.path, new))
