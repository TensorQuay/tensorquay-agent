import os
import tempfile
import unittest

from app import db
from app.ticket import create_ticket, delete_ticket, find_tickets, get_ticket, list_tickets, update_ticket


class TicketTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_ticket(self.path, 18, 'subject-1', 'state-1')
        self.assertEqual(get_ticket(self.path, new)["customer_id"], 18)

    def test_list(self):
        create_ticket(self.path, 18, 'subject-1', 'state-1')
        create_ticket(self.path, 25, 'subject-2', 'state-2')
        self.assertEqual(len(list_tickets(self.path)), 2)

    def test_find(self):
        create_ticket(self.path, 18, 'subject-1', 'state-1')
        create_ticket(self.path, 25, 'subject-2', 'state-2')
        self.assertEqual([row["customer_id"] for row in find_tickets(self.path, 25)],
                         [25])

    def test_update(self):
        new = create_ticket(self.path, 18, 'subject-1', 'state-1')
        self.assertTrue(update_ticket(self.path, new, 25, 'subject-2', 'state-2'))
        self.assertEqual(get_ticket(self.path, new)["state"], 'state-2')

    def test_delete(self):
        new = create_ticket(self.path, 18, 'subject-1', 'state-1')
        self.assertTrue(delete_ticket(self.path, new))
        self.assertIsNone(get_ticket(self.path, new))
        self.assertFalse(delete_ticket(self.path, new))
