import contextlib
import io
import json
import unittest

import report


def run(*argv):
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        report.main(["sales.json", *argv])
    return output.getvalue()


class HeldOut(unittest.TestCase):
    def test_text_is_the_unchanged_default(self):
        expected = "east: 40\nnorth: 150\nsouth: 80\n"
        self.assertEqual(run(), expected)
        self.assertEqual(run("--format", "text"), expected)

    def test_csv(self):
        self.assertEqual(run("--format", "csv"), "region,total\neast,40\nnorth,150\nsouth,80\n")

    def test_json(self):
        self.assertEqual(json.loads(run("--format", "json")),
                         [{"region": "east", "total": 40}, {"region": "north", "total": 150},
                          {"region": "south", "total": 80}])

    def test_other_formats_are_rejected(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            run("--format", "xml")
