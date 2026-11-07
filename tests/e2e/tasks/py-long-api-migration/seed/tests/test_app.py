import unittest

from app import m00, m01


class SmokeTest(unittest.TestCase):
    def test_lines_name_their_endpoint(self):
        self.assertTrue(m00.run().startswith("https://svc.example/e00/"))
        self.assertTrue(m01.run().startswith("https://svc.example/e01/"))
