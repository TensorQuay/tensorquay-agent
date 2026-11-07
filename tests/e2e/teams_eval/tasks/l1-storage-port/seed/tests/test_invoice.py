import os
import tempfile
import unittest

from app import db
from app.invoice import create_invoice, delete_invoice, find_invoices, get_invoice, list_invoices, update_invoice


class InvoiceTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_invoice(self.path, 15, 'number-1', 11)
        self.assertEqual(get_invoice(self.path, new)["order_id"], 15)

    def test_list(self):
        create_invoice(self.path, 15, 'number-1', 11)
        create_invoice(self.path, 22, 'number-2', 18)
        self.assertEqual(len(list_invoices(self.path)), 2)

    def test_find(self):
        create_invoice(self.path, 15, 'number-1', 11)
        create_invoice(self.path, 22, 'number-2', 18)
        self.assertEqual([row["order_id"] for row in find_invoices(self.path, 22)],
                         [22])

    def test_update(self):
        new = create_invoice(self.path, 15, 'number-1', 11)
        self.assertTrue(update_invoice(self.path, new, 22, 'number-2', 18))
        self.assertEqual(get_invoice(self.path, new)["paid"], 18)

    def test_delete(self):
        new = create_invoice(self.path, 15, 'number-1', 11)
        self.assertTrue(delete_invoice(self.path, new))
        self.assertIsNone(get_invoice(self.path, new))
        self.assertFalse(delete_invoice(self.path, new))
