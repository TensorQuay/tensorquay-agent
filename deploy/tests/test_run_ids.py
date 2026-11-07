"""E2E-AUDIT-001's checks: the task's usage rows and the route key's read row must name its run, and never vacuously."""

import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import demo_run_ids

RUN = "run-1"
TASK = "00000000-0000-0000-0000-000000000001"


def rows(**changes):
    base = {"runs": [RUN], "usage": [{"run": RUN, "route": "local"}, {"run": RUN, "route": "local"}],
            "reads": [{"ref": "secrets/local", "kind": "route", "consumer": "local", "run": RUN, "outcome": "ok",
                       "count": 2}],
            "unowned": 0}
    return {"TQA_RUN_ROWS": json.dumps({**base, **changes})}


class RunIdsTests(unittest.TestCase):
    def test_rows_naming_the_run_are_accepted(self):
        with contextlib.redirect_stdout(io.StringIO()) as out:
            demo_run_ids.verify(rows())
        self.assertIn("2 usage rows", out.getvalue())

    def test_a_missing_run_or_empty_evidence_is_refused(self):
        null_read = {"ref": "secrets/local", "kind": "route", "consumer": "local", "run": None, "outcome": "ok",
                     "count": 1}
        ok_read = json.loads(rows()["TQA_RUN_ROWS"])["reads"][0]
        for name, changes in (
                ("no run", {"runs": []}),
                ("no usage", {"usage": []}),
                ("no read", {"reads": []}),
                ("a usage row without a run", {"usage": [{"run": None, "route": "local"}]}),
                ("a usage row of another run", {"usage": [{"run": "run-other", "route": "local"}]}),
                ("only a null-run read", {"reads": [null_read]}),
                ("a null-run read beside the good one", {"reads": [ok_read, null_read]}),
                ("a failed read", {"reads": [{**ok_read, "outcome": "unavailable"}]}),
                ("a read with no task in the window", {"unowned": 1})):
            with self.subTest(name), self.assertRaises(AssertionError):
                demo_run_ids.verify(rows(**changes))

    def test_the_query_names_only_a_validated_task(self):
        with tempfile.TemporaryDirectory() as temporary:
            result = Path(temporary) / "browser.json"
            env = {"TQA_STACK_RESULT": str(result)}
            result.write_text(json.dumps({"conversationId": TASK}))
            with patch.object(demo_run_ids.compose, "command", return_value=Mock(stdout='{"runs": []}\n')) as command, \
                    contextlib.redirect_stdout(io.StringIO()) as out:
                demo_run_ids.record(["docker", "compose"], env)
            statement = command.call_args[0][0][-1]
            self.assertEqual(statement.count("'{}'".format(TASK)), 4)
            self.assertEqual(env["TQA_RUN_ROWS"], '{"runs": []}')
            self.assertIn("Run attribution rows:", out.getvalue())
            result.write_text(json.dumps({"conversationId": "x'; DROP TABLE usage; --"}))
            with self.assertRaises(ValueError):
                demo_run_ids.record(["docker", "compose"], env)


if __name__ == "__main__":
    unittest.main()
