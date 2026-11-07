import contextlib
import io
import unittest

import report


def run(*argv):
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        report.main(["/workspace/sales.json", *argv])
    return output.getvalue()


class HeldOutTest(unittest.TestCase):
    def test_text_is_the_unchanged_default(self):
        expected = "east: 40\nnorth: 150\nsouth: 80\n"
        self.assertEqual(run(), expected)
        self.assertEqual(run("--format", "text"), expected)

    def test_csv(self):
        self.assertEqual(run("--format", "csv"), "region,total\neast,40\nnorth,150\nsouth,80\n")

    def test_other_formats_are_rejected(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            run("--format", "xml")
