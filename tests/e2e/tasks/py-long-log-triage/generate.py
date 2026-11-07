"""Regenerates this task's seed/, check/expected.json and dry-run/ deterministically: python3 generate.py.

The task is long on purpose: forty handler modules and a service log of about 300 KB, in which one handler's
crash must be traced to its code. Only handler 27 divides by a count that can be zero.
"""
import json
import pathlib
import random
import shutil

HERE = pathlib.Path(__file__).resolve().parent
HANDLERS = 40
BROKEN = 27
LOG_LINES = 4000
RATIO_GUARDED = "    if count == 0:\n        return 0.0\n    return round(hits / count, 4)\n"
RATIO_BROKEN = "    return round(hits / count, 4)\n"


def handler(number, guarded):
    body = RATIO_GUARDED if guarded else RATIO_BROKEN
    return (f'"""Handler {number:02d}: request statistics for endpoint /api/e{number:02d}."""\n\n'
            f"WEIGHT = {number % 7 + 1}\n\n\n"
            "def ratio(hits, count):\n"
            '    """Return hits per request, rounded to four places; 0.0 when there were no requests."""\n'
            f"{body}\n\n"
            "def score(values):\n"
            '    """Return the weighted sum of the values."""\n'
            "    return sum(value * WEIGHT for value in values)\n")


def log(rng):
    lines = []
    for index in range(LOG_LINES):
        number = rng.randrange(HANDLERS)
        stamp = f"2026-09-01T{index // 3600 % 24:02d}:{index // 60 % 60:02d}:{index % 60:02d}Z"
        if number == BROKEN and rng.random() < 0.25:
            lines += [f"{stamp} ERROR request={index:05d} handler=app.handlers.h{BROKEN:02d} endpoint=/api/e{BROKEN:02d} status=500",
                      "Traceback (most recent call last):",
                      f'  File "app/handlers/h{BROKEN:02d}.py", line 8, in ratio',
                      "    return round(hits / count, 4)",
                      "ZeroDivisionError: division by zero"]
        else:
            latency = rng.randrange(3, 900)
            lines.append(f"{stamp} INFO request={index:05d} handler=app.handlers.h{number:02d} "
                         f"endpoint=/api/e{number:02d} status=200 latency_ms={latency} bytes={rng.randrange(200, 90000)}")
    return "\n".join(lines) + "\n"


def main():
    rng = random.Random(20260926)
    seed = HERE / "seed"
    shutil.rmtree(seed, ignore_errors=True)
    handlers = seed / "app" / "handlers"
    handlers.mkdir(parents=True)
    (seed / "logs").mkdir()
    (seed / "app" / "__init__.py").write_text("")
    (handlers / "__init__.py").write_text(f"NAMES = [f\"h{{number:02d}}\" for number in range({HANDLERS})]\n")
    for number in range(HANDLERS):
        (handlers / f"h{number:02d}.py").write_text(handler(number, number != BROKEN))
    (seed / "logs" / "service.log").write_text(log(rng))
    (seed / "README.md").write_text("# service\n\nForty request handlers under `app/handlers/`; logs under `logs/`.\n")
    expected = {f"h{number:02d}": {"score": sum(value * (number % 7 + 1) for value in (1, 2, 3)),
                                   "ratio": round(9 / 4, 4)} for number in range(HANDLERS)}
    (HERE / "check" / "expected.json").write_text(json.dumps(expected, indent=1, sort_keys=True) + "\n")
    fixed, wrong = handler(BROKEN, True), handler(BROKEN, False).replace(RATIO_BROKEN, "    return None if count == 0 else round(hits / count, 4)\n")
    for name, content, final in (("solve", fixed, f"Handler {BROKEN} now returns 0.0 when there were no requests."),
                                 ("wrong", wrong, f"Handler {BROKEN} no longer divides by zero.")):
        scenario = {"model": "fixture", "responses": [
            {"events": [{"type": "tool_call", "name": "grep", "arguments": {"pattern": "ERROR", "path": "logs"}, "split_into": 1}],
             "usage": {"prompt_tokens": 600, "completion_tokens": 40}},
            {"events": [{"type": "tool_call", "name": "write_file", "arguments": {"path": f"app/handlers/h{BROKEN:02d}.py", "content": content, "create_parents": False}, "split_into": 2}],
             "usage": {"prompt_tokens": 2400, "completion_tokens": 150}},
            {"events": [{"type": "text", "text": final}], "usage": {"prompt_tokens": 2700, "completion_tokens": 20}}]}
        (HERE / "dry-run" / f"{name}.json").write_text(json.dumps(scenario, indent=1) + "\n")


if __name__ == "__main__":
    main()
