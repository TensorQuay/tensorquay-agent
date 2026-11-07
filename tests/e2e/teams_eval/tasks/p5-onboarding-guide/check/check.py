import os
import re
import shlex
import subprocess
import sys
from checklib import fail, read, run, sections

def check(workspace):
    text = read(workspace, "repo/ONBOARDING.md")
    headings = " ".join(heading.lower() for heading, _ in sections(text))
    for needed, words in (("setup", ("setup", "set up", "install")), ("layout", ("layout", "structure")),
                          ("tests", ("test",)), ("making a change", ("change", "contribut"))):
        if not any(word in headings for word in words):
            fail(f"ONBOARDING.md has no section on {needed}")
    blocks = re.findall(r"```(?:sh|bash|shell|console)?\n(.*?)```", text, re.S)
    commands = [line.strip().lstrip("$ ").strip() for block in blocks for line in block.splitlines()]
    commands = [command for command in commands if command.startswith("python3 ")]
    if len(commands) < 2 or not any("unittest" in command for command in commands):
        fail("ONBOARDING.md gives fewer than two python3 commands, or none that runs the tests")
    repo = workspace / "repo"
    environment = dict(os.environ, PYTHONPATH=str(repo), PYTHONDONTWRITEBYTECODE="1")
    for command in commands:
        argv = [sys.executable] + shlex.split(command)[1:]
        result = subprocess.run(argv, cwd=repo, capture_output=True, text=True, timeout=60, env=environment)
        if result.returncode != 0:
            fail(f"the guide's command fails: {command}", result.stdout[-800:] + result.stderr[-800:])
    return f"the sections are there and its {len(commands)} commands run"

run(check)
