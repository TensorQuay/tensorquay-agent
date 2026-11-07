import os
import tempfile
import unittest

from app import db
from app.lead import create_lead, delete_lead, find_leads, get_lead, list_leads, update_lead


class LeadTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_lead(self.path, 'email-1', 'source-1', 12)
        self.assertEqual(get_lead(self.path, new)["email"], 'email-1')

    def test_list(self):
        create_lead(self.path, 'email-1', 'source-1', 12)
        create_lead(self.path, 'email-2', 'source-2', 19)
        self.assertEqual(len(list_leads(self.path)), 2)

    def test_find(self):
        create_lead(self.path, 'email-1', 'source-1', 12)
        create_lead(self.path, 'email-2', 'source-2', 19)
        self.assertEqual([row["email"] for row in find_leads(self.path, 'email-2')],
                         ['email-2'])

    def test_update(self):
        new = create_lead(self.path, 'email-1', 'source-1', 12)
        self.assertTrue(update_lead(self.path, new, 'email-2', 'source-2', 19))
        self.assertEqual(get_lead(self.path, new)["score"], 19)

    def test_delete(self):
        new = create_lead(self.path, 'email-1', 'source-1', 12)
        self.assertTrue(delete_lead(self.path, new))
        self.assertIsNone(get_lead(self.path, new))
        self.assertFalse(delete_lead(self.path, new))
