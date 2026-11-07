import unittest

from pagination import page, page_count


class PageCountTest(unittest.TestCase):
    def test_exact_multiple(self):
        self.assertEqual(page_count(9, 3), 3)

    def test_partial_last_page(self):
        self.assertEqual(page_count(10, 3), 4)

    def test_rejects_empty_pages(self):
        with self.assertRaises(ValueError):
            page_count(10, 0)


class PageTest(unittest.TestCase):
    def test_last_page_is_partial(self):
        self.assertEqual(page(list(range(10)), 4, 3), [9])
