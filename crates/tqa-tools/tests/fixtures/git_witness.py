"""Record only the disposable process fixture's explicitly supplied arguments and environment."""
import json
import os
from pathlib import Path
import sys

environment = dict(os.environ)
# The macOS Python runtime adds this process-local key after exec.
# The Rust test also checks the exact environment supplied before exec.
if sys.platform == "darwin" and "__CF_USER_TEXT_ENCODING" in environment:
    encoding = environment.pop("__CF_USER_TEXT_ENCODING").split(":")
    assert len(encoding) == 3 and int(encoding[0], 16) == os.getuid()
    assert all(0 <= int(value, 16) <= 0xFFFFFFFF for value in encoding)
Path(__file__).with_suffix(".json").write_text(json.dumps({"args": sys.argv[1:], "env": environment}))
