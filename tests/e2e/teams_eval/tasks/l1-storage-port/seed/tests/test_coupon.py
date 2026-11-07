import os
import tempfile
import unittest

from app import db
from app.coupon import create_coupon, delete_coupon, find_coupons, get_coupon, list_coupons, update_coupon


class CouponTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_coupon(self.path, 'code-1', 14, 13)
        self.assertEqual(get_coupon(self.path, new)["code"], 'code-1')

    def test_list(self):
        create_coupon(self.path, 'code-1', 14, 13)
        create_coupon(self.path, 'code-2', 21, 20)
        self.assertEqual(len(list_coupons(self.path)), 2)

    def test_find(self):
        create_coupon(self.path, 'code-1', 14, 13)
        create_coupon(self.path, 'code-2', 21, 20)
        self.assertEqual([row["code"] for row in find_coupons(self.path, 'code-2')],
                         ['code-2'])

    def test_update(self):
        new = create_coupon(self.path, 'code-1', 14, 13)
        self.assertTrue(update_coupon(self.path, new, 'code-2', 21, 20))
        self.assertEqual(get_coupon(self.path, new)["active"], 20)

    def test_delete(self):
        new = create_coupon(self.path, 'code-1', 14, 13)
        self.assertTrue(delete_coupon(self.path, new))
        self.assertIsNone(get_coupon(self.path, new))
        self.assertFalse(delete_coupon(self.path, new))
