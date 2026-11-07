"""Prime the Cargo artifact once before timed standalone tests on macOS."""

import os
from pathlib import Path
import re
import selectors
import signal
import socket
import subprocess
import sys
import tempfile
import time


def discover_binary(search_path):
    """Select exactly one executable from nextest's active artifact search path."""
    candidates = set()
    for directory in search_path.split(os.pathsep):
        if not directory:
            continue
        candidate = Path(directory) / "tqa-fake-model"
        if candidate.is_file() and os.access(str(candidate), os.X_OK):
            candidates.add(candidate.resolve())
    if len(candidates) != 1:
        raise ValueError("standalone priming requires exactly one executable artifact")
    return candidates.pop()


def remaining(deadline):
    """Bound the entire priming operation, including graceful shutdown."""
    seconds = deadline - time.monotonic()
    if seconds <= 0:
        raise TimeoutError("standalone priming deadline elapsed")
    return seconds


def read_ready(stream, deadline):
    """Read a bounded, complete ready line without a polling interval."""
    data = bytearray()
    with selectors.DefaultSelector() as events:
        events.register(stream, selectors.EVENT_READ)
        while b"\n" not in data:
            if not events.select(remaining(deadline)):
                raise TimeoutError("standalone readiness deadline elapsed")
            chunk = os.read(stream.fileno(), 129 - len(data))
            if not chunk:
                raise RuntimeError("server exited before complete readiness")
            data.extend(chunk)
            if len(data) > 128:
                raise RuntimeError("oversize standalone readiness")
    match = re.fullmatch(br"listening 127\.0\.0\.1:([1-9][0-9]{0,4})\n", data)
    if match is None or int(match.group(1)) > 65535:
        raise RuntimeError("invalid standalone readiness")
    return int(match.group(1))


def prime(binary, scenario):
    """Start, observe the bound port, and gracefully reap one untimed fixture."""
    deadline = time.monotonic() + 120
    with tempfile.TemporaryFile() as errors:
        child = subprocess.Popen(
            [str(binary), "--listen", "127.0.0.1:0", "--scenario", str(scenario)],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=errors,
        )
        try:
            port = read_ready(child.stdout, deadline)
            with socket.create_connection(("127.0.0.1", port), timeout=remaining(deadline)):
                pass
            child.send_signal(signal.SIGTERM)
            status = child.wait(timeout=remaining(deadline))
            if status != 0:
                raise RuntimeError("standalone priming exit status {}".format(status))
        except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
            errors.seek(0)
            detail = errors.read(4096).decode("utf-8", errors="replace").strip()
            raise RuntimeError("{}: {}".format(error, detail)) from error
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=5)
            child.stdout.close()


def main():
    """Use nextest's profile-specific linker path, including coverage builds."""
    search_path = os.environ.get("NEXTEST_DYLD_FALLBACK_LIBRARY_PATH", "")
    binary = discover_binary(search_path)
    if "\n" in str(binary) or "\r" in str(binary):
        raise ValueError("standalone artifact path cannot contain a newline")
    environment_file = Path(os.environ["NEXTEST_ENV"])
    scenario = Path(__file__).resolve().parents[1] / "scenarios" / "demo.json"
    prime(binary, scenario)
    with environment_file.open("a", encoding="utf-8") as output:
        output.write("TQA_PRIMED_FAKE_MODEL={}\n".format(binary))


if __name__ == "__main__":
    try:
        main()
    except (KeyError, OSError, RuntimeError, ValueError) as error:
        print("standalone priming failed: {}".format(error), file=sys.stderr)
        sys.exit(1)
