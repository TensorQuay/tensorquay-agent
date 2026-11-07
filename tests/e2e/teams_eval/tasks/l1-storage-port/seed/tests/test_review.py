import os
import tempfile
import unittest

from app import db
from app.review import create_review, delete_review, find_reviews, get_review, list_reviews, update_review


class ReviewTests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_review(self.path, 17, 18, 12)
        self.assertEqual(get_review(self.path, new)["product_id"], 17)

    def test_list(self):
        create_review(self.path, 17, 18, 12)
        create_review(self.path, 24, 25, 19)
        self.assertEqual(len(list_reviews(self.path)), 2)

    def test_find(self):
        create_review(self.path, 17, 18, 12)
        create_review(self.path, 24, 25, 19)
        self.assertEqual([row["product_id"] for row in find_reviews(self.path, 24)],
                         [24])

    def test_update(self):
        new = create_review(self.path, 17, 18, 12)
        self.assertTrue(update_review(self.path, new, 24, 25, 19))
        self.assertEqual(get_review(self.path, new)["stars"], 19)

    def test_delete(self):
        new = create_review(self.path, 17, 18, 12)
        self.assertTrue(delete_review(self.path, new))
        self.assertIsNone(get_review(self.path, new))
        self.assertFalse(delete_review(self.path, new))
