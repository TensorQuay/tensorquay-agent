import os
import tempfile
import unittest

from app import db
from app.refund import create_refund, delete_refund, find_refunds, get_refund, list_refunds, update_refund


class RefundTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_refund(self.path, 15, 19, 'reason-1')
        self.assertEqual(get_refund(self.path, new)["order_id"], 15)

    def test_list(self):
        create_refund(self.path, 15, 19, 'reason-1')
        create_refund(self.path, 22, 26, 'reason-2')
        self.assertEqual(len(list_refunds(self.path)), 2)

    def test_find(self):
        create_refund(self.path, 15, 19, 'reason-1')
        create_refund(self.path, 22, 26, 'reason-2')
        self.assertEqual([row["order_id"] for row in find_refunds(self.path, 22)],
                         [22])

    def test_update(self):
        new = create_refund(self.path, 15, 19, 'reason-1')
        self.assertTrue(update_refund(self.path, new, 22, 26, 'reason-2'))
        self.assertEqual(get_refund(self.path, new)["reason"], 'reason-2')

    def test_delete(self):
        new = create_refund(self.path, 15, 19, 'reason-1')
        self.assertTrue(delete_refund(self.path, new))
        self.assertIsNone(get_refund(self.path, new))
        self.assertFalse(delete_refund(self.path, new))
