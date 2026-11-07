import unittest

from client import fetch_with_retry


class ClientTest(unittest.TestCase):
    def test_returns_the_result(self):
        self.assertEqual(fetch_with_retry(lambda: 42, sleep=lambda seconds: None), 42)
