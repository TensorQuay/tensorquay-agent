import json
import tempfile
import unittest
from pathlib import Path

from pipeline import run

HERE = Path(__file__).resolve().parent


class EndToEnd(unittest.TestCase):
    def test_other_logs_through_all_four_stages(self):
        cases = json.loads((HERE / "cases.json").read_text())
        for case in cases:
            with tempfile.TemporaryDirectory() as scratch:
                logs, out = Path(scratch) / "logs", Path(scratch) / "out"
                logs.mkdir()
                for name, lines in case["logs"].items():
                    (logs / name).write_text("\n".join(lines) + "\n", encoding="utf-8")
                run.main(str(logs), str(out), case["week"])
                self.assertEqual(json.loads((out / "weekly.json").read_text()), case["figures"], case["name"])
                self.assertTrue((out / "weekly.md").read_text().count("|") > 10)
