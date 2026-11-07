import unittest

from ledgerlite.book import Book


class BookTest(unittest.TestCase):
    def test_balance_sums_entries(self):
        book = Book()
        book.add(5, "in")
        book.add(-2, "out")
        self.assertEqual(book.balance(), 3)
