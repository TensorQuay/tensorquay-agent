#!/usr/bin/env python3
"""Regenerate the deterministic WEB-E2E-018 fixture; no external inputs."""
import json
from pathlib import Path


def call(name, **arguments):
    if name == "write_file":
        arguments.setdefault("create_parents", False)
    return {"type": "tool_call", "name": name, "arguments": arguments, "split_into": 4}


def reply(reasoning, text, *calls):
    return {
        "events": [{"type": "reasoning", "text": reasoning}, {"type": "text", "text": text}, *calls],
        "usage": {"prompt_tokens": 320, "completion_tokens": 100, "reasoning_tokens": 20},
        "delay_ms": 65,
    }


def scenario():
    tests = (
        "import unittest\nfrom arithmetic import add\n\n"
        "class ArithmeticTests(unittest.TestCase):\n"
        "    def test_addition(self):\n        self.assertEqual(add(19, 23), 42)\n\n"
        "    def test_zero(self):\n        self.assertEqual(add(0, 0), 0)\n"
    )
    responses = [
        reply("A small arithmetic module makes the test failure easy to explain.",
              "I'll create a module and a short README, then check the behavior.\n\n",
              call("write_file", path="demo/README.md", content="# Arithmetic demo\nDraft: checks pending.\n", create_parents=True),
              call("write_file", path="demo/arithmetic.py", content="def add(left, right):\n    return left - right\n")),
        reply("The implementation needs a regression test for ordinary inputs and zero.",
              "The module is in place. I'll read it and add two focused tests.\n\n",
              call("read_file", path="demo/arithmetic.py"),
              call("write_file", path="demo/test_arithmetic.py", content=tests)),
        reply("Running the tests before editing will expose the incorrect operation.",
              "I'll list the project files and run the first test pass.\n\n",
              call("glob", pattern="demo/*.py"),
              call("shell", command="python3 -B -m unittest discover -s demo -v")),
        reply("The ordinary addition case failed because the function subtracts its second input.",
              "One test failed as expected. I'll locate the subtraction and replace it with addition.\n\n",
              call("grep", pattern="return", path="demo/arithmetic.py", output_mode="content"),
              call("edit_file", path="demo/arithmetic.py", edits=[{"old_text": "return left - right", "new_text": "return left + right"}])),
        reply("The exact replacement is complete; the same test command should now pass.",
              "I'll rerun both tests and update the README with the result.\n\n",
              call("shell", command="python3 -B -m unittest discover -s demo -v"),
              call("edit_file", path="demo/README.md", edits=[{"old_text": "Draft: checks pending.", "new_text": "Verified: two arithmetic tests pass."}])),
        reply("A final read and test discovery check will confirm the files match the summary.",
              "Both tests passed. I'll check the final documentation and the test names.\n\n",
              call("read_file", path="demo/README.md"),
              call("grep", pattern="def test_", path="demo/test_arithmetic.py", output_mode="content")),
    ]
    answer = final_answer()
    responses.append({
        "events": [{"type": "text", "text": answer[index:index + 64]} for index in range(0, len(answer), 64)],
        "usage": {"prompt_tokens": 2200, "completion_tokens": 800},
        "delay_ms": 65,
    })
    return {"model": "fixture", "responses": responses}


def final_answer():
    evidence = json.dumps({"checks": [{"name": "addition-and-zero", "result": "passed", "iteration": n} for n in range(8)]})
    return """## A working arithmetic example

Created a small Python project in `demo/` and checked it with two repeatable tests. The addition function now returns
the sum of its inputs. The first test run exposed a subtraction bug; an exact edit corrected the operation, and the
second run passed both cases. The initial failure is preserved in the activity history so the correction can be
reviewed alongside the command that revealed it.

- `demo/arithmetic.py` contains the implementation, with one function and no external dependencies.
- `demo/test_arithmetic.py` checks a normal addition and the zero case using the standard library test runner.
- `demo/README.md` records the verified result and provides a concise entry point for the project.

| Check | First run | Final run |
| --- | --- | --- |
| Add 19 and 23 | Failed: returned -4 | Passed: returned 42 |
| Add zero to zero | Passed | Passed |
| Project documentation | Checks pending | Two tests verified |

The implementation is intentionally small enough to inspect in one screen. The tests assert observable return values;
they do not repeat the implementation as their oracle. Keeping the same command before and after the edit makes the
result easy to reproduce, and the unchanged zero case checks that the correction did not disturb that boundary.

```python
def add(left, right):
    return left + right

assert add(19, 23) == 42
assert add(0, 0) == 0
```

## Verification and next steps

Run `python3 -B -m unittest discover -s demo -v` from the workspace root. This uses the Python standard library and does
not require a package download. The command exits successfully when both tests pass. If a later edit breaks addition,
the failure identifies the expected and actual values instead of silently accepting an incorrect result.

The final file read confirmed that the README describes the verified state. A search also found both test methods,
and the file listing showed the implementation and test file together. These checks give separate evidence for the
behavior, the project structure and the documentation. The workspace retains every file for further inspection.

The compact record below uses one deliberately long line so it remains easy to copy as a complete JSON value. Its
repeated entries illustrate a batch report; the table above describes the two actual tests that were executed.

```json
""" + evidence + """
```

For a larger module, add cases for negative inputs and mixed numeric types before extending the implementation.
The current change is complete: the regression is fixed, both tests pass, and the documentation matches the files.
"""


if __name__ == "__main__":
    destination = Path(__file__).with_name("demo-long.json")
    destination.write_text(json.dumps(scenario(), indent=2) + "\n", encoding="utf-8")
