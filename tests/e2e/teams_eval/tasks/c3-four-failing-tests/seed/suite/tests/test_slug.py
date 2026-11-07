import unittest

from pkg.slug import slugify


class SlugTest(unittest.TestCase):
    def test_single_dashes(self):
        self.assertEqual(slugify("Hello,  World!"), "hello-world")
