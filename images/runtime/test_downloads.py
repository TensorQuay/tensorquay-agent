"""REL-U-012: the image builds' downloads survive a dropped connection. Every download a checksum guards goes through
one shared form, images/fetch.py in a RUN that checks the file's SHA-256 after it, so a reset, a refused connection or
a stalled transfer is tried again and only a lasting failure stops the build, naming the URL and the last error. A
wrong file still fails at once: the checksum is the caller's, after the download. Run with unittest discovery in this
directory; no Docker."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
from pathlib import Path
import re
import socket
import ssl
import struct
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock
import urllib.error

ROOT = Path(__file__).resolve().parents[2]
FETCH = ROOT / "images" / "fetch.py"
MOUNT = "--mount=type=bind,source=images/fetch.py,target=/tmp/fetch.py"
SHARED = "RUN " + MOUNT + " python3 /tmp/fetch.py "
# The same form for a build whose context is its own folder (the kernel's): the helper's byte-identical copy there.
FOLDER_MOUNT = MOUNT.replace("source=images/fetch.py", "source=fetch.py")
FOLDER_SHARED = "RUN " + FOLDER_MOUNT + " python3 /tmp/fetch.py "
KERNEL = ROOT / "images" / "rootfs" / "kernel"


def shared(line):
    """Whether a RUN line starts with the shared form, from the repository root or from the build's own folder."""
    return line.startswith(SHARED) or line.startswith(FOLDER_SHARED)


def dockerfiles():
    """Every Dockerfile of the image builds: images/ and deploy/, at any depth."""
    found = []
    for top in ("images", "deploy"):
        found += [path for path in (ROOT / top).rglob("Dockerfile*")
                  if path.is_file() and not path.name.endswith(".dockerignore") and "node_modules" not in path.parts]
    return sorted(found)


def instructions(text):
    """The Dockerfile's instructions, continuations joined and comment lines dropped."""
    lines = [line for line in text.splitlines() if not line.lstrip().startswith("#")]
    return [line.strip() for line in re.sub(r"\\\n\s*", " ", "\n".join(lines)).splitlines() if line.strip()]


DOWNLOADER = re.compile(r"\b(?:curl|wget)\s+(?:-|https?://|\$)|urllib|urlretrieve|urlopen|http\.client|\brequests\b")


def download_problems(text):
    """What in a Dockerfile downloads outside the shared form, one line per instruction."""
    problems = []
    for line in instructions(text):
        keyword = line.split(None, 1)[0].upper()
        if keyword in ("ADD", "COPY") and re.search(r"\s(?:https?|ftp)://", line):
            problems.append("downloads with {} and no retry: {}".format(keyword, line[:120]))
        elif keyword == "RUN" and "fetch.py" in line:
            if not shared(line):
                problems.append("fetch.py not in the shared form ({}): {}".format(SHARED.strip(), line[:120]))
            elif "sha256sum -c" not in line.split("fetch.py", 2)[-1]:
                problems.append("a download with no sha256sum -c after it in the same RUN: " + line[:120])
            elif DOWNLOADER.search(line.replace(MOUNT, "").replace(FOLDER_MOUNT, "").replace("python3 /tmp/fetch.py", "")):
                problems.append("downloads beside the shared form: " + line[:120])
        elif keyword == "RUN" and DOWNLOADER.search(line):
            problems.append("downloads outside the shared form: " + line[:120])
    return problems


class SharedForm(unittest.TestCase):
    GOOD = (SHARED + "https://example.org/a.tgz /tmp/a.tgz \\\n    && sha256sum -c /tmp/a.sha256 \\\n"
            "    && tar -xzf /tmp/a.tgz\n")

    def test_every_image_download_uses_the_shared_form(self):
        files = dockerfiles()
        names = [str(path.relative_to(ROOT)) for path in files]
        for expected in ("images/runtime/Dockerfile", "images/vmm/Dockerfile", "images/rootfs/kernel/Dockerfile"):
            self.assertIn(expected, names)
        for path in files:
            with self.subTest(dockerfile=str(path.relative_to(ROOT))):
                self.assertEqual(download_problems(path.read_text()), [])

    def test_the_guarded_downloads_are_all_there(self):
        # The class, counted: Go, npm and its two replaced packages, the kernel and its configuration, Firecracker and
        # its published checksum file. A download that leaves the shared form would leave this count.
        fetched = 0
        for path in dockerfiles():
            for line in instructions(path.read_text()):
                if shared(line):
                    fetched += len(re.findall(r"(?:https://\S+|\"\$\(cat [^)]*\)\"|\$\{?\w*URL\}?)\s+/\S+",
                                              line.split("&&", 1)[0]))
        self.assertEqual(fetched, 8)

    def test_the_shared_form_passes(self):
        self.assertEqual(download_problems(self.GOOD), [])
        self.assertEqual(download_problems(self.GOOD.replace(SHARED, FOLDER_SHARED)), [])

    def test_each_copy_of_the_helper_is_the_same_bytes(self):
        # A build whose context is its own folder carries a copy; a copy that drifts would retry differently.
        copies = [path for path in (ROOT / "images").rglob("fetch.py") if path != FETCH]
        self.assertIn(KERNEL / "fetch.py", copies)
        for copy in copies:
            with self.subTest(copy=str(copy.relative_to(ROOT))):
                self.assertTrue(copy.read_bytes() == FETCH.read_bytes(),
                                "{} differs from {}: copy it again".format(copy.relative_to(ROOT), FETCH.relative_to(ROOT)))

    def test_a_helper_mount_names_a_file_in_its_build_context(self):
        # images/fetch.py from the repository root; fetch.py only where the Dockerfile's own folder holds the copy.
        for path in dockerfiles():
            for line in instructions(path.read_text()):
                if line.startswith(FOLDER_SHARED):
                    with self.subTest(dockerfile=str(path.relative_to(ROOT))):
                        self.assertTrue((path.parent / "fetch.py").is_file())

    def test_the_kernel_builds_from_its_own_folder(self):
        # A fix release changes no command (decided on 10 Nov 2026): the kernel's context stays images/rootfs/kernel,
        # so everything its Dockerfile copies or mounts from the context is in that folder.
        for line in instructions((KERNEL / "Dockerfile").read_text()):
            words = line.split()
            if words[0] == "COPY" and not any(word.startswith("--from=") for word in words):
                for source in [word for word in words[1:-1] if not word.startswith("--")]:
                    with self.subTest(copy=source):
                        self.assertTrue((KERNEL / source).is_file(), line)
            for source in re.findall(r"--mount=type=bind,source=([^,\s]+)", line):
                with self.subTest(mount=source):
                    self.assertTrue((KERNEL / source).is_file(), line)
        release = (ROOT / "tools" / "release" / "release.py").read_text()
        self.assertRegex(release, r'"type=local,dest=\{\}"\.format\(built\),\s*self\.source / "images/rootfs/kernel"\]')

    def test_each_other_form_is_refused(self):
        for text in ("ADD --checksum=sha256:{} https://example.org/a.tgz /tmp/a.tgz\n".format("a" * 64),
                     "ADD https://example.org/a.tgz /tmp/a.tgz\n",
                     "ADD --checksum=sha256:{} \\\n    https://example.org/a.tgz /tmp/a.tgz\n".format("a" * 64),
                     "RUN python3 -c 'import urllib.request; urllib.request.urlretrieve(\"https://example.org/a\", \"a\")'\n",
                     "RUN curl -fsSL https://example.org/a.tgz -o /tmp/a.tgz && sha256sum -c a.sha256\n",
                     "RUN wget https://example.org/a.tgz\n",
                     "RUN python3 /tmp/fetch.py https://example.org/a.tgz /tmp/a.tgz && sha256sum -c a.sha256\n",
                     SHARED + "https://example.org/a.tgz /tmp/a.tgz && tar -xzf /tmp/a.tgz\n",
                     "RUN sha256sum -c a.sha256 && " + SHARED[4:] + "https://example.org/a.tgz /tmp/a.tgz\n",
                     self.GOOD.replace("&& tar", "&& curl -o /tmp/b https://example.org/b && tar")):
            with self.subTest(text=text):
                self.assertEqual(len(download_problems(text)), 1, download_problems(text))

    def test_a_package_named_curl_and_a_comment_are_no_download(self):
        self.assertEqual(download_problems("RUN apt-get install -y jq curl unzip wget tree\n"
                                           "# ADD https://example.org/a.tgz was replaced\n"), [])


class Answers(BaseHTTPRequestHandler):
    """Serves BODY; each request takes the next of the server's plan: 'reset', 'stall', 'short', a status, or 'ok'."""

    def do_GET(self):
        plan = self.server.plan
        step = plan.pop(0) if plan else self.server.then
        self.server.requests += 1
        if step == "reset":
            self.connection.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
            self.close_connection = True
            self.connection.close()
        elif step == "stall":
            time.sleep(3)
            self.close_connection = True
        elif step == "trickle":
            self.send_response(200)
            self.send_header("Content-Length", str(len(self.server.body)))
            self.end_headers()
            for byte in self.server.body[:12]:
                self.wfile.write(bytes([byte]))
                self.wfile.flush()
                time.sleep(0.25)
            self.close_connection = True
        elif step == "steady":
            # A slow but healthy link: about 51 KB/s from the first byte to the last, two seconds in all.
            self.send_response(200)
            self.send_header("Content-Length", str(len(self.server.body)))
            self.end_headers()
            for start in range(0, len(self.server.body), 2048):
                self.wfile.write(self.server.body[start:start + 2048])
                self.wfile.flush()
                time.sleep(0.04)
        elif step == "short":
            self.send_response(200)
            self.send_header("Content-Length", str(len(self.server.body)))
            self.end_headers()
            self.wfile.write(self.server.body[:10])
            self.wfile.flush()
            self.close_connection = True
        elif isinstance(step, int):
            self.send_error(step)
        else:
            self.send_response(200)
            self.send_header("Content-Length", str(len(self.server.body)))
            self.end_headers()
            self.wfile.write(self.server.body)

    def log_message(self, *args):
        pass


class Fetch(unittest.TestCase):
    BODY = bytes(range(256)) * 400

    def serve(self, plan, then="ok"):
        server = ThreadingHTTPServer(("127.0.0.1", 0), Answers)
        server.plan, server.then, server.requests, server.body = list(plan), then, 0, self.BODY
        server.daemon_threads = True
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return server, "http://127.0.0.1:{}/go.linux-amd64.tar.gz".format(server.server_address[1])

    def fetch(self, *pairs, tries=3):
        work = tempfile.TemporaryDirectory()
        self.addCleanup(work.cleanup)
        started = time.monotonic()
        done = subprocess.run([sys.executable, "-I", str(FETCH), "--tries", str(tries), "--pause", "0.1",
                               "--stall", "1", "--window", "1", "--min-rate", "2000", *[str(item).replace("@", work.name) for item in pairs]],
                              capture_output=True, text=True, timeout=60, check=False)
        return done, Path(work.name), time.monotonic() - started

    def test_a_dropped_first_connection_is_tried_again_and_the_file_is_whole(self):
        for drop in ("reset", "short", 503, 429):
            with self.subTest(first=drop):
                server, url = self.serve([drop])
                done, work, _ = self.fetch(url, "@/go.tar.gz")
                self.assertEqual(done.returncode, 0, done.stderr)
                self.assertEqual((work / "go.tar.gz").read_bytes(), self.BODY)
                self.assertEqual(server.requests, 2)
                self.assertIn("trying again", done.stderr)

    def test_a_lasting_failure_names_the_url_and_the_last_error_after_every_try(self):
        server, url = self.serve([], then="reset")
        done, work, _ = self.fetch(url, "@/go.tar.gz", tries=3)
        self.assertNotEqual(done.returncode, 0)
        self.assertEqual(server.requests, 3)
        last = done.stderr.strip().splitlines()[-1]
        self.assertIn(url, last)
        self.assertIn("3 tries", last)
        self.assertRegex(last, r"(?i)reset")
        self.assertEqual(list(work.iterdir()), [], "no partial file is left")

    def test_a_body_cut_short_on_every_try_leaves_no_partial_file(self):
        server, url = self.serve([], then="short")
        done, work, _ = self.fetch(url, "@/go.tar.gz", tries=2)
        self.assertNotEqual(done.returncode, 0)
        self.assertEqual(server.requests, 2)
        self.assertIn(url, done.stderr.strip().splitlines()[-1])
        self.assertEqual(list(work.iterdir()), [])

    def test_a_stalled_transfer_ends_after_its_stall_time_and_is_tried_again(self):
        server, url = self.serve([], then="stall")
        done, work, elapsed = self.fetch(url, "@/go.tar.gz", tries=2)
        self.assertNotEqual(done.returncode, 0)
        self.assertEqual(server.requests, 2)
        self.assertIn(url, done.stderr.strip().splitlines()[-1])
        self.assertRegex(done.stderr.strip().splitlines()[-1], r"(?i)timed out")
        self.assertLess(elapsed, 2.9 * 2, "each try ends after its stall time, not when the server gives up")

    def test_a_trickling_transfer_ends_when_its_rate_falls_below_the_floor(self):
        # Bytes keep coming, so no single read waits long: the rate over the last window ends the try.
        server, url = self.serve(["trickle"], then="ok")
        done, work, elapsed = self.fetch(url, "@/go.tar.gz", tries=2)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(server.requests, 2)
        self.assertRegex(done.stderr, r"(?i)too slow")
        self.assertEqual((work / "go.tar.gz").read_bytes(), self.BODY)
        self.assertLess(elapsed, 2.9)

    def test_a_slow_but_steady_transfer_finishes_however_long_it_takes(self):
        # Longer than a window, and well above the rate floor: it is never cut off (a 150 MB archive at 200 KB/s).
        server, url = self.serve(["steady"])
        done, work, elapsed = self.fetch(url, "@/go.tar.gz", tries=1)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(server.requests, 1)
        self.assertEqual((work / "go.tar.gz").read_bytes(), self.BODY)
        self.assertGreater(elapsed, 1.5, "the transfer took longer than one window")

    def test_an_answer_that_is_no_network_error_fails_at_once(self):
        for status in (404, 403, 410):
            with self.subTest(status=status):
                server, url = self.serve([status], then="ok")
                done, work, _ = self.fetch(url, "@/go.tar.gz")
                self.assertNotEqual(done.returncode, 0)
                self.assertEqual(server.requests, 1)
                self.assertIn(url, done.stderr)
                self.assertIn(str(status), done.stderr)
                self.assertEqual(list(work.iterdir()), [])

    def test_several_downloads_in_one_call_each_land_whole(self):
        server, url = self.serve(["reset", "ok", "reset"])
        done, work, _ = self.fetch(url, "@/a.tgz", url, "@/b.tgz")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual((work / "a.tgz").read_bytes(), self.BODY)
        self.assertEqual((work / "b.tgz").read_bytes(), self.BODY)
        self.assertEqual(server.requests, 4)

    def test_a_url_without_a_destination_is_refused_before_any_download(self):
        server, url = self.serve([])
        done, _, _ = self.fetch(url)
        self.assertEqual(done.returncode, 2)
        self.assertEqual(server.requests, 0)


def load_fetch():
    """images/fetch.py as a module, so a test can stand in for the network below it."""
    spec = importlib.util.spec_from_file_location("fetch_under_test", FETCH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Answer:
    """A stand-in for urlopen's answer: `body`, and, when `drop` is set, a TLS drop after its first 1000 bytes."""

    def __init__(self, body, drop):
        self.body, self.drop, self.at = body, drop, 0
        self.headers = {"Content-Length": str(len(body))}

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False

    def read1(self, size):
        if self.drop and self.at >= 1000:
            raise ssl.SSLEOFError(8, "EOF occurred in violation of protocol (_ssl.c:2427)")
        chunk = self.body[self.at:self.at + min(size, 1000)]
        self.at += len(chunk)
        return chunk


class Tls(unittest.TestCase):
    BODY = bytes(range(256)) * 40
    URL = "https://example.org/firecracker-v1.17.0-x86_64.tgz"

    def run_fetch(self, answers, tries=3):
        fetch = load_fetch()
        work = tempfile.TemporaryDirectory()
        self.addCleanup(work.cleanup)
        dest = str(Path(work.name) / "firecracker.tgz")
        calls = []

        def urlopen(url, timeout):
            calls.append(url)
            answer = answers.pop(0)
            if isinstance(answer, Exception):
                raise answer
            return answer

        with mock.patch.object(fetch.urllib.request, "urlopen", urlopen), mock.patch.object(fetch.time, "sleep"):
            problem = fetch.fetch(self.URL, dest, tries, 0.0, 60.0)
        return problem, Path(dest), calls

    def test_a_tls_drop_during_the_body_is_tried_again_and_the_file_is_whole(self):
        problem, dest, calls = self.run_fetch([Answer(self.BODY, drop=True), Answer(self.BODY, drop=False)])
        self.assertIsNone(problem)
        self.assertEqual(len(calls), 2)
        self.assertEqual(dest.read_bytes(), self.BODY)
        self.assertEqual(list(dest.parent.iterdir()), [dest], "no partial file is left")

    def test_a_certificate_refused_at_connect_ends_as_before_naming_the_url(self):
        refused = urllib.error.URLError(ssl.SSLCertVerificationError(1, "certificate verify failed"))
        problem, dest, calls = self.run_fetch([refused] * 3)
        self.assertEqual(len(calls), 3)
        self.assertIn(self.URL, problem)
        self.assertIn("certificate verify failed", problem)
        self.assertFalse(dest.exists())


if __name__ == "__main__":
    unittest.main()
