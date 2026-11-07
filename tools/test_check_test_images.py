"""SBX-F-034, TK-I-002 (the run check): what a test run must not leave on the engine."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_test_images  # noqa: E402

KEY, OTHER = "0011223344556677", "8899aabbccddeeff"


class Leftovers(unittest.TestCase):
    def test_one_probe_image_per_variant_and_nothing_else_is_clean(self):
        images = ["tqa-fc-probe:test-" + KEY, "tqa-fc-probe:derived-" + KEY, "tqa-fc-probe:secret-env-" + KEY,
                  "tqa-runtime:base", "tqa-gateway:abc"]
        self.assertEqual(check_test_images.leftovers(images), [])

    def test_a_second_key_of_one_variant_is_left_behind(self):
        images = ["tqa-fc-probe:test-" + KEY, "tqa-fc-probe:test-" + OTHER, "tqa-fc-probe:derived-" + KEY]
        self.assertEqual(check_test_images.leftovers(images),
                         ["tqa-fc-probe:test-" + KEY, "tqa-fc-probe:test-" + OTHER])

    def test_any_ext_check_image_is_left_behind(self):
        for image in ("tqa-extcheck-tki002-binary:796734", "tqa-extcheck-mcp-1-2:latest"):
            with self.subTest(image=image):
                self.assertEqual(check_test_images.leftovers([image]), [image])


if __name__ == "__main__":
    unittest.main()
