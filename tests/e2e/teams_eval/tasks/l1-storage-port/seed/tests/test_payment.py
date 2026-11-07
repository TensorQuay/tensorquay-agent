import os
import tempfile
import unittest

from app import db
from app.payment import create_payment, delete_payment, find_payments, get_payment, list_payments, update_payment


class PaymentTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_payment(self.path, 15, 'method-1', 19)
        self.assertEqual(get_payment(self.path, new)["order_id"], 15)

    def test_list(self):
        create_payment(self.path, 15, 'method-1', 19)
        create_payment(self.path, 22, 'method-2', 26)
        self.assertEqual(len(list_payments(self.path)), 2)

    def test_find(self):
        create_payment(self.path, 15, 'method-1', 19)
        create_payment(self.path, 22, 'method-2', 26)
        self.assertEqual([row["order_id"] for row in find_payments(self.path, 22)],
                         [22])

    def test_update(self):
        new = create_payment(self.path, 15, 'method-1', 19)
        self.assertTrue(update_payment(self.path, new, 22, 'method-2', 26))
        self.assertEqual(get_payment(self.path, new)["amount_cents"], 26)

    def test_delete(self):
        new = create_payment(self.path, 15, 'method-1', 19)
        self.assertTrue(delete_payment(self.path, new))
        self.assertIsNone(get_payment(self.path, new))
        self.assertFalse(delete_payment(self.path, new))
