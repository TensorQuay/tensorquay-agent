"""G-10 install: the pinned Gitleaks download survives a transient server error and still fails at once on a missing
release. CI installs Gitleaks from its GitHub release before the secrets scan; on 9 Nov 2026 a single 504 answer to
that download failed a pull request's whole rust job. The tests run the script's own curl command against a local
release host."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import threading
import unittest

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "tools" / "install-gitleaks.sh"
RELEASES = "https://github.com/gitleaks/gitleaks/releases/download/"
ARCHIVE = b"the release archive\n"
PATH = "/v8.30.1/gitleaks.tar.gz"


def download_command():
    """The script's curl command with its continuation lines, exactly as the script runs it."""
    lines = SCRIPT.read_text().splitlines()
    start = next(index for index, line in enumerate(lines) if line.startswith("curl "))
    command = []
    for line in lines[start:]:
        command.append(line)
        if not line.endswith("\\"):
            return "\n".join(command)
    raise AssertionError("the curl command in {} never ends".format(SCRIPT))


class Answer(BaseHTTPRequestHandler):
    """Answers each request with the next planned status, the archive on a 200; 404 once the plan runs out."""

    def do_GET(self):
        status = self.server.plan.pop(0) if self.server.plan else 404
        self.server.served.append((self.path, status))
        body = ARCHIVE if status == 200 else b"not this time\n"
        self.send_response(status)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def install(plan):
    """Run the download against a local host that answers with `plan`; return curl's exit code, what the host served
    and the archive written (None when no file was written)."""
    command = download_command()
    if command.count(RELEASES) != 1:
        raise AssertionError("the release URL in {} changed; update RELEASES".format(SCRIPT))
    with tempfile.TemporaryDirectory() as directory, ThreadingHTTPServer(("127.0.0.1", 0), Answer) as host:
        host.plan, host.served = list(plan), []
        threading.Thread(target=host.serve_forever, daemon=True).start()
        try:
            local = "http://127.0.0.1:{}/".format(host.server_address[1])
            script = "set -euo pipefail\nversion=8.30.1\narchive=gitleaks.tar.gz\ndownload={}\n{}\n".format(
                shlex.quote(directory), command.replace(RELEASES, local))
            # No proxy and no user curlrc: only the script's own options decide.
            env = {key: value for key, value in os.environ.items() if "proxy" not in key.lower()}
            env.update(CURL_HOME=directory, XDG_CONFIG_HOME=directory)
            run = subprocess.run(["bash", "-c", script], capture_output=True, text=True, timeout=120, env=env,
                                 check=False)
        finally:
            host.shutdown()
        archive = Path(directory) / "gitleaks.tar.gz"
        return run.returncode, host.served, archive.read_bytes() if archive.exists() else None


class Download(unittest.TestCase):
    def test_a_transient_504_is_tried_again_and_the_archive_arrives(self):
        self.assertEqual(install([504, 200]), (0, [(PATH, 504), (PATH, 200)], ARCHIVE))

    def test_a_missing_release_fails_at_once_without_trying_again(self):
        code, served, written = install([404])
        self.assertEqual((code, served), (22, [(PATH, 404)]))  # 22: curl --fail on an HTTP error answer
        self.assertIn(written, (None, b""))


if __name__ == "__main__":
    unittest.main()
