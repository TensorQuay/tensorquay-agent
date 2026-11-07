"""Runs the workspace's tests under a line tracer and writes the executed lines of the measured
modules as JSON: python3 covrun.py WORKSPACE OUT MODULE..."""
import json
import sys
import unittest
from pathlib import Path

workspace = Path(sys.argv[1]).resolve()
targets = {str((workspace / name).resolve()): name for name in sys.argv[3:]}
hit = {path: set() for path in targets}


def local(frame, event, arg):
    if event == "line":
        hit[frame.f_code.co_filename].add(frame.f_lineno)
    return local


def tracer(frame, event, arg):
    if frame.f_code.co_filename in hit:
        hit[frame.f_code.co_filename].add(frame.f_lineno)
        return local
    return None


sys.path.insert(0, str(workspace))
sys.settrace(tracer)
suite = unittest.defaultTestLoader.discover(str(workspace / "tests"), top_level_dir=str(workspace))
result = unittest.TextTestRunner(stream=sys.stderr).run(suite)
sys.settrace(None)
with open(sys.argv[2], "w") as handle:
    json.dump({"passed": result.wasSuccessful(), "ran": result.testsRun,
               "hit": {targets[path]: sorted(lines) for path, lines in hit.items()}}, handle)
