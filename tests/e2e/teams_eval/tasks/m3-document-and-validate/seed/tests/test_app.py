import unittest

from svc.app import dispatch


class AppTest(unittest.TestCase):
    def test_health(self):
        self.assertEqual(dispatch("GET", "/health"),
                         (200, {"status": "ok"}))
