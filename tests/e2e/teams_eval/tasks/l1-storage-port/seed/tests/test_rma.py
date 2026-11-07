import os
import tempfile
import unittest

from app import db
from app.rma import create_rma, delete_rma, find_rmas, get_rma, list_rmas, update_rma


class RmaTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_rma(self.path, 15, 'state-1', 12)
        self.assertEqual(get_rma(self.path, new)["order_id"], 15)

    def test_list(self):
        create_rma(self.path, 15, 'state-1', 12)
        create_rma(self.path, 22, 'state-2', 19)
        self.assertEqual(len(list_rmas(self.path)), 2)

    def test_find(self):
        create_rma(self.path, 15, 'state-1', 12)
        create_rma(self.path, 22, 'state-2', 19)
        self.assertEqual([row["order_id"] for row in find_rmas(self.path, 22)],
                         [22])

    def test_update(self):
        new = create_rma(self.path, 15, 'state-1', 12)
        self.assertTrue(update_rma(self.path, new, 22, 'state-2', 19))
        self.assertEqual(get_rma(self.path, new)["items"], 19)

    def test_delete(self):
        new = create_rma(self.path, 15, 'state-1', 12)
        self.assertTrue(delete_rma(self.path, new))
        self.assertIsNone(get_rma(self.path, new))
        self.assertFalse(delete_rma(self.path, new))
