import unittest

from pagination import page, page_count


class HeldOutTest(unittest.TestCase):
    def test_counts(self):
        cases = [(0, 5, 0), (1, 5, 1), (5, 5, 1), (6, 5, 2), (99, 10, 10), (100, 10, 10)]
        for total, size, expected in cases:
            self.assertEqual(page_count(total, size), expected, (total, size))

    def test_size_must_be_positive(self):
        for size in (0, -1):
            with self.assertRaises(ValueError):
                page_count(3, size)

    def test_pages_unchanged(self):
        self.assertEqual(page(list(range(7)), 1, 3), [0, 1, 2])
        self.assertEqual(page(list(range(7)), 3, 3), [6])
        with self.assertRaises(ValueError):
            page([], 0, 3)
