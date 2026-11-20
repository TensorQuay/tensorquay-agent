"""INST-U-001: deploy/install.sh's installer (deploy/tools/install.py) without Docker. Each question's validation and
default; --yes; the refusal on an existing install; the files it writes, byte for byte the install guide's step 2
block's for the same answers (both call deploy/tools/install_files.py; a frozen copy of the 18.0 block pins the
bytes); the route probe's verdict and its known fix; the https:// proxy line; and the redaction: no password, token or
key in anything it prints. A scripted `System` answers the commands and HTTP calls a real host would."""

import contextlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "deploy" / "tools"))

import install  # noqa: E402
import install_files  # noqa: E402
import shell  # noqa: E402

HERE = Path(__file__).resolve().parent
POSTGRES = re.compile(r"^(TQA_POSTGRES_PASSWORD|TQA_BOOTSTRAP_TOKEN)=[0-9a-f]{64}$", re.M)
ADMIN_PASSWORD = "correct horse battery"
# deploy/pull-images.sh's output, as it printed it for 18.0.0 on this kind of host.
EXPORTS = (
    "== 10:00:00 pulling ghcr.io/tensorquay/tensorquay-agent-runtime:18.1.0\n"
    "== 10:00:01 pulling ghcr.io/tensorquay/tensorquay-agent-gateway:18.1.0\n"
    "== 10:00:01 pulling ghcr.io/tensorquay/tensorquay-agent-web:18.1.0\n"
    "== 10:00:01 done. For step 1:\n"
    "export TQA_RUNTIME_IMAGE='sha256:" + "a" * 64 + "'\n"
    "export TQA_GATEWAY_IMAGE='ghcr.io/tensorquay/tensorquay-agent-gateway@sha256:" + "b" * 64 + "'\n"
    "export TQA_WEB_IMAGE='ghcr.io/tensorquay/tensorquay-agent-web@sha256:" + "c" * 64 + "'\n"
)


class Failed(Exception):
    pass


class FakeIO:
    """Scripted answers, in order; every printed line kept."""

    def __init__(self, answers=(), secrets=()):
        self.answers = list(answers)
        self.secrets = list(secrets)
        self.prompts = []
        self.lines = []

    def ask(self, prompt):
        self.prompts.append(prompt)
        if not self.answers:
            raise AssertionError("asked with no scripted answer: " + prompt)
        return self.answers.pop(0)

    def secret(self, prompt):
        self.prompts.append(prompt)
        if not self.secrets:
            raise AssertionError("asked a secret with no scripted answer: " + prompt)
        return self.secrets.pop(0)

    def say(self, line):
        self.lines.append(line)

    @property
    def text(self):
        return "\n".join(self.lines)


class FakeSystem:
    """Answers the installer's commands and HTTP calls; records each."""

    def __init__(self, *, rootless=False, probe=None, bootstrap=201, up_fails=None, ready_after=1,
                 busy_ports=(), detail=None, group_fails=False, workstation=None):
        self.commands = []
        self.environments = []
        self.requests = []
        self.rootless = rootless
        self.group_fails = group_fails
        # The ID of a tqa-runtime:workstation already on the host (another build's), or None.
        self.workstation = workstation
        self.probe = probe
        self.bootstrap = bootstrap
        self.up_fails = up_fails
        self.ready_after = ready_after
        self.busy_ports = set(busy_ports)
        self.detail = detail
        self.slept = 0.0

    def run(self, args, *, env=None, capture=False, timeout=None):
        self.commands.append(list(args))
        self.environments.append(env)
        if args == ["docker", "info", "--format", "{{.SecurityOptions}}"]:
            return "[name=seccomp,profile=builtin" + (" name=rootless" if self.rootless else "") + "]\n"
        if args[:2] == ["docker", "run"] and "stat" in args:
            # As toybox's stat answers: the group of the one file named, and a refusal without one.
            if self.group_fails or args[-4:] != ["stat", "-c", "%g", "/var/run/docker.sock"]:
                raise shell.CommandFailed(args, 1, "stat: Needs 1 argument (see \"stat --help\")")
            return "998\n"
        if args and args[0].endswith("pull-images.sh"):
            return EXPORTS
        if args[:2] == ["docker", "run"] and "chown" in args:
            # As chown answers: every path it names must be in the folder it mounts.
            mount = next(arg for arg in args if arg.startswith("type=bind,source="))
            source = Path(mount.split("source=", 1)[1].split(",", 1)[0])
            for name in args[args.index("chown") + 3:]:
                if not (source / name.removeprefix("/install/")).exists():
                    raise shell.CommandFailed(args, 1, "chown: {}: No such file or directory".format(name))
            return ""
        if args[:3] == ["docker", "build", "-f"]:
            if not (ROOT / args[3]).is_file():
                raise shell.CommandFailed(args, 1, "failed to read dockerfile: open {}: no such file".format(args[3]))
            return ""
        if args[:2] == ["docker", "compose"]:
            # Compose reads both files before anything else.
            for flag, base in (("--env-file", None), ("-f", ROOT)):
                path = Path(args[args.index(flag) + 1])
                if not (base / path if base else path).is_file():
                    raise shell.CommandFailed(args, 1, "couldn't find {}".format(path))
            if "up" in args and self.up_fails is not None:
                raise shell.CommandFailed(args, 1, self.up_fails(env))
            return ""
        if args == ["docker", "image", "inspect", "--format", "{{.Id}}", "tqa-runtime:workstation"]:
            if self.workstation is None:
                raise shell.CommandFailed(args, 1, "Error response from daemon: No such image: tqa-runtime:workstation")
            return self.workstation + "\n"
        if args[:3] == ["docker", "run", "-d"] or args[:2] == ["docker", "pull"]:
            return ""
        # A command a real host would answer in its own way: the fake must know it, never answer it blindly.
        raise AssertionError("a command the scripted host doesn't know: {}".format(args))

    def http(self, method, url, *, body=None, headers=None, timeout=None):
        self.requests.append((method, url, body, dict(headers or {})))
        if url.endswith("/readyz"):
            self.ready_after -= 1
            return (200 if self.ready_after <= 0 else 503), None
        if url.endswith("/api/v1/auth/bootstrap"):
            return self.bootstrap, ({"code": "conflict"} if self.bootstrap >= 400 else {"id": "u"})
        if url.endswith("/readyz?detail=1"):
            return 200, self.detail or {"ready": True, "checks": {"database": True, "secret_store": True,
                                                                    "backend_preflight": True, "image_present": True}}
        if url.endswith("/api/v1/admin/routes/local/probe"):
            if isinstance(self.probe, Exception):
                raise self.probe
            return 200, self.probe
        raise AssertionError("unexpected request " + url)

    def port_free(self, port):
        return port not in self.busy_ports

    def sleep(self, seconds):
        self.slept += seconds


def probe(state="verified", failure=None, total_ms=2100):
    return {"probe": {"state": state, "probed_at": "2026-11-17T10:00:00Z", "route_version": 1, "observed": {
        "ok": state == "verified", "streaming": True, "tool_call": "passed", "round_trip": "passed",
        "usage": {"reported": True, "cached": False, "reasoning": False}, "reasoning_accepted": None,
        "first_byte_ms": 300, "total_ms": total_ms, "tokens": {"prompt": 10, "completion": 5},
        "failure": failure, "warnings": []}}}


def private_file(directory, name, text, mode=0o600):
    path = Path(directory) / name
    path.write_text(text)
    path.chmod(mode)
    return path


class Case(unittest.TestCase):
    def setUp(self):
        self.home = Path(tempfile.mkdtemp(prefix="tqa-install-"))
        self.addCleanup(shutil.rmtree, self.home, True)
        self.install_dir = self.home / "data" / "tensorquay"
        self.password_file = private_file(self.home, "admin-password", ADMIN_PASSWORD + "\n")

    def run_installer(self, argv, io_=None, system=None):
        io_ = io_ or FakeIO()
        system = system or FakeSystem()
        code = install.main(["--install-dir", str(self.install_dir)] + argv, io=io_, system=system,
                            repository=ROOT)
        return code, io_, system

    def yes(self, *more):
        return ["--yes", "--fake-model", "--admin-email", "ana@example.org",
                "--admin-password-file", str(self.password_file), *more]

    def assert_nothing_written(self):
        self.assertFalse(self.install_dir.exists(), "the install folder was created")


class Questions(Case):
    """Each question's check and default, before anything is written."""

    def test_the_address_defaults_to_this_computer_on_8080(self):
        address = install.check_address("", port_free=lambda port: True)
        self.assertEqual((address.origin, address.port, address.https), ("http://localhost:8080", 8080, False))

    def test_an_address_on_this_computer_needs_its_own_port(self):
        address = install.check_address("http://localhost:8081", port_free=lambda port: True)
        self.assertEqual((address.origin, address.port), ("http://localhost:8081", 8081))
        for text in ("http://localhost", "http://localhost:80", "http://localhost:70000"):
            with self.subTest(text), self.assertRaisesRegex(install.Refused, "port"):
                install.check_address(text, port_free=lambda port: True)

    def test_an_https_address_is_served_on_a_local_port_behind_the_proxy(self):
        address = install.check_address("https://agent.example.org/", port_free=lambda port: True)
        self.assertEqual((address.origin, address.port, address.https), ("https://agent.example.org", 8080, True))
        self.assertEqual(install.check_address("https://agent.example.org:8443", port_free=lambda port: True).origin,
                         "https://agent.example.org:8443")

    def test_other_addresses_are_refused_in_words(self):
        for text in ("localhost:8080", "ftp://agent.example.org", "http://agent.example.org:8080",
                     "https://agent.example.org/agent", "https://agent.example.org/?a=1", "https://",
                     "http://user@localhost:8080"):
            with self.subTest(text), self.assertRaises(install.Refused) as refused:
                install.check_address(text, port_free=lambda port: True)
            self.assertIn("http://localhost:8080", str(refused.exception))

    def test_a_port_in_use_is_refused(self):
        with self.assertRaisesRegex(install.Refused, "8080 is already in use"):
            install.check_address("", port_free=lambda port: port != 8080)
        with self.assertRaisesRegex(install.Refused, "9000 is already in use"):
            install.check_address("https://agent.example.org", port_free=lambda port: port != 9000, port=9000)

    def test_the_model_server_address(self):
        self.assertEqual(install.check_model_url("https://models.example/v1"), "https://models.example/v1")
        for text in ("models.example/v1", "ftp://models.example", "http://", "https://models.example/v1 x"):
            with self.subTest(text), self.assertRaises(install.Refused):
                install.check_model_url(text)

    def test_a_local_model_server_is_told_apart_as_the_gateway_tells_it(self):
        for url in ("http://localhost:11434/v1", "http://127.0.0.1:8000/v1", "http://host.docker.internal:8000/v1",
                    "http://ollama:11434/v1", "http://box.local:8000/v1", "http://gpu.internal/v1",
                    "http://172.16.0.1:8000/v1", "http://10.0.0.5/v1", "http://100.64.0.1/v1",
                    "http://[fd00::1]:8000/v1"):
            with self.subTest(url):
                self.assertTrue(install.is_local(url))
        for url in ("https://models.example/v1", "https://api.provider.example/v1", "http://1.0.0.1/v1"):
            with self.subTest(url):
                self.assertFalse(install.is_local(url))

    def test_the_model_name_and_context(self):
        self.assertEqual(install.check_model_name(" example-model "), "example-model")
        for text in ("", "a\nb", "x" * 201):
            with self.subTest(text), self.assertRaises(install.Refused):
                install.check_model_name(text)
        self.assertEqual(install.check_context(""), 32768)
        self.assertEqual(install.check_context("65536"), 65536)
        for text in ("abc", "512", "10000001", "-1"):
            with self.subTest(text), self.assertRaisesRegex(install.Refused, "32768"):
                install.check_context(text)

    def test_the_key_file_is_checked_as_step_2_checks_it(self):
        good = private_file(self.home, "model-key", "the-key\n")
        self.assertEqual(install.check_key_file(str(good), repository=ROOT), good)
        cases = {
            "missing": (self.home / "nothing", "doesn't exist"),
            "shared": (private_file(self.home, "shared", "k\n", 0o644), "chmod 600"),
            "empty": (private_file(self.home, "empty", "\n"), "empty"),
            "large": (private_file(self.home, "large", "k" * 8193), "8192"),
            "in the checkout": (ROOT / "README.md", "outside this checkout"),
        }
        for name, (path, words) in cases.items():
            with self.subTest(name), self.assertRaisesRegex(install.Refused, words):
                install.check_key_file(str(path), repository=ROOT)

    def test_the_administrator(self):
        self.assertEqual(install.check_email(" ana@example.org "), "ana@example.org")
        for text in ("", "ana", "ana@", "@example.org", "ana@example", "a na@example.org"):
            with self.subTest(text), self.assertRaises(install.Refused):
                install.check_email(text)
        install.check_password("x" * 12)
        with self.assertRaisesRegex(install.Refused, "12 characters"):
            install.check_password("x" * 11)

    def test_a_refused_answer_is_asked_again(self):
        key = private_file(self.home, "model-key", "the-key\n")
        answers = ["localhost:8080", "", "models.example/v1", "https://models.example/v1", "", "example-model",
                   str(self.home / "nothing"), str(key), "ana", "ana@example.org"]
        code, out, system = self.run_installer([], FakeIO(answers, ["short", ADMIN_PASSWORD]),
                                               FakeSystem(probe=probe()))
        self.assertEqual(code, 0, out.text)
        self.assertEqual(out.prompts[0], "Where will people open TensorQuay Agent? [http://localhost:8080]")
        self.assertIn("Which model server? Its chat-completions address, or Enter for the test model:", out.prompts)
        # A hosted server isn't asked for its context; a refused answer says why and asks again.
        self.assertNotIn("Context", " ".join(out.prompts))
        self.assertIn("Password (12 characters or more, not shown):", out.prompts)
        self.assertEqual(sum("12 characters" in line for line in out.lines), 1)

    def test_a_local_model_server_is_asked_its_context(self):
        answers = ["", "http://172.16.0.1:8000/v1", "qwen", "", "65536", "ana@example.org"]
        code, out, _ = self.run_installer([], FakeIO(answers, [ADMIN_PASSWORD]), FakeSystem(probe=probe()))
        self.assertEqual(code, 0, out.text)
        self.assertIn("  Context window in tokens [32768]:", out.prompts)
        self.assertIn("context_window=65536\n", (self.install_dir / "models.toml").read_text())


class Yes(Case):
    """--yes takes every default; the administrator has none, so it must be given."""

    def test_yes_with_the_test_model_installs_and_starts_without_a_question(self):
        code, out, system = self.run_installer(self.yes())
        # Nothing was asked: the scripted IO has no answers and would have failed.
        self.assertEqual((code, out.prompts), (0, []), out.text)
        names = [" ".join(command) for command in system.commands]
        order = ["deploy/pull-images.sh", "docker build -f images/fake-model/Dockerfile", "chown -R 1000:1000",
                 "up -d --no-build --wait", "docker run -d --name tqa-fake-model --network tqa_control"]
        positions = [next(i for i, name in enumerate(names) if part in name) for part in order]
        self.assertEqual(positions, sorted(positions), names)
        bootstrap = next(request for request in system.requests if request[1].endswith("/auth/bootstrap"))
        self.assertEqual(bootstrap[1], "http://127.0.0.1:8080/api/v1/auth/bootstrap")
        self.assertEqual(bootstrap[3]["Origin"], "http://localhost:8080")
        sent = json.loads(bootstrap[2])
        token = re.search(r"^TQA_BOOTSTRAP_TOKEN=(\w+)$", (self.install_dir / "compose.env").read_text(), re.M)[1]
        self.assertEqual(sent, {"email": "ana@example.org", "password": ADMIN_PASSWORD, "token": token})
        self.assertFalse(any(url.endswith("/probe") for _, url, _, _ in system.requests), "the test model is probed")
        self.assertIn("The test model needs no check.", out.lines)
        self.assertEqual(out.lines[-1], "Ready. Open http://localhost:8080 and sign in as ana@example.org.")
        # The stack's start is said as such, and "Ready" only once the administrator exists (it read "Ready in 0 s."
        # first, a person's test run of 20 Nov 2026).
        self.assertIn("Started.", out.lines)
        self.assertLess(out.lines.index("Started."), len(out.lines) - 1)
        self.assertEqual([line for line in out.lines if line.startswith("Ready")], [out.lines[-1]])
        models = (self.install_dir / "models.toml").read_text()
        self.assertIn('base_url="http://tqa-fake-model:8080/v1"\nmodel="fixture"\n', models)

    def test_yes_without_an_administrator_refuses_before_anything(self):
        for argv in (["--yes", "--fake-model"], ["--yes", "--fake-model", "--admin-email", "ana@example.org"]):
            with self.subTest(argv):
                code, out, system = self.run_installer(argv)
                self.assertEqual(code, 2)
                self.assertIn("--admin-email and --admin-password-file", out.text)
                self.assertEqual(system.commands, [])
                self.assert_nothing_written()

    def test_a_password_is_never_taken_as_a_value(self):
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            install.parse(["--admin-password", "x"])

    def test_yes_with_a_model_server_takes_its_answers_from_flags(self):
        key = private_file(self.home, "model-key", "the-key\n")
        code, out, system = self.run_installer(
            ["--yes", "--model-url", "https://models.example/v1", "--model", "example-model", "--key-file", str(key),
             "--admin-email", "ana@example.org", "--admin-password-file", str(self.password_file)],
            system=FakeSystem(probe=probe()))
        self.assertEqual(code, 0, out.text)
        self.assertIn('"secrets/local" = "the-key"', (self.install_dir / "secrets.toml").read_text())

    def test_upgrade_is_refused_in_words(self):
        code, out, system = self.run_installer(["--upgrade"])
        self.assertEqual(code, 2)
        self.assertIn("doesn't upgrade", out.text)
        # The guide's section that has the steps (deploy/README.md), by its name.
        self.assertIn('"Upgrading to a new release" in deploy/README.md', out.text)
        self.assertEqual(system.commands, [])


class ExistingInstall(Case):
    def test_an_existing_install_is_refused_and_left_as_it_was(self):
        self.install_dir.mkdir(parents=True)
        env = self.install_dir / "compose.env"
        env.write_text("TQA_HTTP_PORT=8081\nTQA_GATEWAY_IMAGE=ghcr.io/tensorquay/tensorquay-agent-gateway@sha256:"
                       + "b" * 64 + "\n")
        before = env.read_bytes()
        code, out, system = self.run_installer(self.yes())
        self.assertEqual(code, 2)
        self.assertEqual(env.read_bytes(), before)
        self.assertIn('"Upgrading to a new release" in deploy/README.md', out.text)
        self.assertEqual(sorted(path.name for path in self.install_dir.iterdir()), ["compose.env"])
        self.assertEqual(system.commands, [])
        self.assertIn("There is already an installation in " + str(self.install_dir), out.text)
        self.assertIn("http://localhost:8081", out.text)
        self.assertIn("deploy/pull-images.sh", out.text)
        self.assertIn("docker compose --env-file " + str(env) + " -f deploy/compose/docker.yml down", out.text)

    def test_a_folder_already_there_is_refused_too(self):
        self.install_dir.mkdir(parents=True)
        (self.install_dir / "notes.txt").write_text("mine")
        code, out, system = self.run_installer(self.yes())
        self.assertEqual(code, 2)
        self.assertIn("isn't empty", out.text)
        self.assertEqual((self.install_dir / "notes.txt").read_text(), "mine")
        self.assertEqual(system.commands, [])


def masked(directory):
    """Every entry of an installation with its mode and bytes; the random values masked (the key's length, the two
    hex secrets) and the installation's own path written as <root>."""
    files = {".": (oct(stat.S_IMODE(Path(directory).stat().st_mode)), None)}
    for path in sorted(Path(directory).rglob("*")):
        relative = str(path.relative_to(directory))
        mode = oct(stat.S_IMODE(path.stat().st_mode))
        if path.is_dir():
            files[relative + "/"] = (mode, None)
            continue
        data = path.read_bytes()
        if relative == "install.key":
            data = b"<%d random bytes>" % len(data)
        elif relative == "compose.env":
            data = POSTGRES.sub(lambda match: match[1] + "=<64 hex>", data.decode()).encode()
        files[relative] = (mode, data.replace(str(directory).encode(), b"<root>"))
    return files


def guide_block(readme):
    """Step 2's shell block from the install guide."""
    section = readme.split("### 2. Create the installation", 1)[1]
    return section.split("```sh\n", 1)[1].split("```", 1)[0]


class FilesAsTheGuideMakesThem(Case):
    """The installer's files are step 2's for the same answers, byte for byte (random values masked)."""

    def answers(self, root, key):
        return {
            "TQA_INSTALL_DIR": str(root), "TQA_PROVIDER_KEY_FILE": str(key),
            "TQA_MODEL_BASE_URL": "https://models.example/v1", "TQA_MODEL": "example-model",
            "TQA_MODEL_CONTEXT": "32768", "TQA_RUNTIME_IMAGE": "sha256:" + "a" * 64,
            "TQA_GATEWAY_IMAGE": "ghcr.io/tensorquay/tensorquay-agent-gateway@sha256:" + "b" * 64,
            "TQA_WEB_IMAGE": "ghcr.io/tensorquay/tensorquay-agent-web@sha256:" + "c" * 64, "TQA_HTTP_PORT": "8080",
        }

    def by_block(self, block, root, key, more=()):
        env = {"PATH": os.environ["PATH"], "HOME": str(self.home), **self.answers(root, key), **dict(more)}
        done = subprocess.run(["bash", "-euc", block], cwd=ROOT, env=env, capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 0, done.stderr)
        return done.stdout

    def by_installer(self, root, key):
        self.install_dir = root
        code, out, _ = self.run_installer(
            ["--yes", "--model-url", "https://models.example/v1", "--model", "example-model", "--key-file", str(key),
             "--admin-email", "ana@example.org", "--admin-password-file", str(self.password_file)],
            system=FakeSystem(probe=probe()))
        self.assertEqual(code, 0, out.text)
        return out

    def test_the_guide_block_the_frozen_18_0_block_and_the_installer_agree(self):
        key = private_file(self.home, "model-key", "the-key\n")
        guide = self.home / "guide"
        frozen = self.home / "frozen"
        mine = self.home / "installer"
        printed = self.by_block(guide_block((ROOT / "deploy" / "README.md").read_text()), guide, key)
        self.by_block((HERE / "install_block_18_0.sh").read_text(), frozen, key)
        self.by_installer(mine, key)
        self.assertEqual(printed, "created " + str(guide) + "; compose.env also sets: TQA_GATEWAY_IMAGE, "
                                  "TQA_WEB_IMAGE, TQA_HTTP_PORT\n")
        self.assertEqual(masked(guide), masked(frozen))
        self.assertEqual(masked(mine), masked(frozen))

    def test_the_guide_block_calls_the_shared_function(self):
        self.assertIn("deploy/tools/install_files.py", guide_block((ROOT / "deploy" / "README.md").read_text()))

    def test_the_shared_function_refuses_as_the_block_did(self):
        key = private_file(self.home, "model-key", "the-key\n")
        root = self.home / "x"
        env = self.answers(root, key)
        install_files.create(env, ROOT)
        with self.assertRaises(FileExistsError):
            install_files.create(env, ROOT)
        for name, change in (("key in the checkout", {"TQA_PROVIDER_KEY_FILE": str(ROOT / "README.md")}),
                             ("install in the checkout", {"TQA_INSTALL_DIR": str(ROOT / "deploy" / "x")}),
                             ("context", {"TQA_MODEL_CONTEXT": "lots"})):
            with self.subTest(name), self.assertRaises(SystemExit):
                install_files.create({**env, "TQA_INSTALL_DIR": str(self.home / name.replace(" ", "-")), **change},
                                     ROOT)
        self.assertFalse((ROOT / "deploy" / "x").exists())

    def test_secrets_stay_in_their_two_files(self):
        key = private_file(self.home, "model-key", "the-key\n")
        out = self.by_installer(self.home / "installer", key)
        for path in (self.home / "installer").rglob("*"):
            if path.is_file():
                data = path.read_bytes()
                with self.subTest(path.name):
                    self.assertNotIn(ADMIN_PASSWORD.encode(), data)
                    if path.name != "secrets.toml":
                        self.assertNotIn(b"the-key", data)
        self.assertNotIn("the-key", out.text)

    def test_rootless_docker_is_detected_and_written_as_step_1_does(self):
        code, out, system = self.run_installer(self.yes(), system=FakeSystem(rootless=True))
        self.assertEqual(code, 0, out.text)
        compose = (self.install_dir / "compose.env").read_text()
        self.assertRegex(compose, r"(?m)^TQA_DOCKER_SOCKET=/.+/docker\.sock$")
        self.assertIn("TQA_DOCKER_GID=998\n", compose)

    def test_the_socket_s_group_is_read_as_the_guide_reads_it(self):
        # Step 1's block reads the group of the socket mounted at /var/run/docker.sock; the installer runs that line.
        self.assertIn("stat -c %g /var/run/docker.sock", (ROOT / "deploy" / "README.md").read_text())
        code, out, system = self.run_installer(self.yes(), system=FakeSystem(rootless=True))
        self.assertEqual(code, 0, out.text)
        stat_line = next(command for command in system.commands if "stat" in command)
        self.assertEqual(stat_line[-4:], ["stat", "-c", "%g", "/var/run/docker.sock"])
        self.assertIn(",target=/var/run/docker.sock,readonly", " ".join(stat_line))

    def test_the_shell_s_own_settings_never_reach_its_commands(self):
        # Compose takes a variable from the shell over --env-file: a shell that followed the guide's steps by hand
        # (export TQA_INSTALL_DIR=..., the image lines) would start another installation's files or images.
        leftovers = {"TQA_INSTALL_DIR": "/elsewhere", "TQA_GATEWAY_IMAGE": "sha256:" + "0" * 64,
                     "TQA_HTTP_PORT": "9999", "COMPOSE_PROJECT_NAME": "another", "COMPOSE_FILE": "other.yml"}
        with mock.patch.dict(os.environ, dict(leftovers, TQA_IMAGE_REGISTRY="mirror.example/tensorquay")):
            code, out, system = self.run_installer(self.yes(), system=FakeSystem(rootless=True))
        self.assertEqual(code, 0, out.text)
        for args, env in zip(system.commands, system.environments):
            self.assertIsNotNone(env, args)
            self.assertFalse(set(leftovers) & set(env), args)
            self.assertEqual(env.get("PATH"), os.environ.get("PATH"), args)
        pull = system.environments[[args[0].endswith("pull-images.sh") for args in system.commands].index(True)]
        self.assertEqual(pull["TQA_IMAGE_REGISTRY"], "mirror.example/tensorquay")

    def test_another_builds_workstation_on_the_host_is_named(self):
        # The gateway takes a tqa-runtime:workstation already on the host as this installation's Workstation (the
        # guide's "Which image an environment runs"): without --workstation the installer says which image that is.
        other = "sha256:" + "d246e473fd79" + "0" * 52
        code, out, system = self.run_installer(self.yes(), system=FakeSystem(workstation=other))
        self.assertEqual(code, 0, out.text)
        self.assertIn("This computer already has a Workstation image from another build (sha256:d246e473fd79)",
                      out.text)
        self.assertIn("Settings › Environments", out.text)
        self.assertLess(out.text.index("another build"), out.text.index("Starting…"))

    def test_no_line_without_another_workstation(self):
        code, out, _ = self.run_installer(self.yes(), system=FakeSystem())
        self.assertEqual(code, 0, out.text)
        self.assertNotIn("another build", out.text)

    def test_no_line_when_the_pull_brings_this_releases_workstation(self):
        # With --workstation the pull tags this release's own image, so there's nothing to say.
        code, out, _ = self.run_installer(self.yes("--workstation"), system=FakeSystem(
            workstation="sha256:" + "e" * 64))
        self.assertEqual(code, 0, out.text)
        self.assertNotIn("another build", out.text)

    def test_a_failed_detection_names_the_step_and_writes_nothing(self):
        code, out, system = self.run_installer(self.yes(), system=FakeSystem(rootless=True, group_fails=True))
        self.assertEqual(code, 3, out.text)
        self.assertIn("Reading rootless Docker's socket group failed.", out.text)
        self.assertIn("stat: Needs 1 argument", out.text)
        self.assertNotIn("Traceback", out.text)
        self.assertFalse(self.install_dir.exists())

    def test_a_second_install_takes_its_own_names(self):
        code, out, system = self.run_installer(self.yes("--name", "tensorquay-staging", "--port", "8081",
                                                        "--network-pool", "10.214.0.0/16"))
        self.assertEqual(code, 0, out.text)
        compose = (self.install_dir / "compose.env").read_text()
        for line in ("COMPOSE_PROJECT_NAME=tensorquay-staging", "TQA_GATEWAY_CONTAINER=tqa-gateway-tensorquay-staging",
                     "TQA_SANDBOX_NETWORK=tqa-sandboxes-tensorquay-staging", "TQA_HTTP_PORT=8081"):
            self.assertIn(line + "\n", compose)
        self.assertTrue((self.install_dir / "gateway.toml").read_text().endswith(
            '[sandbox.network_pool]\ncidr = "10.214.0.0/16"\nprefix = 29\n'))
        names = [" ".join(command) for command in system.commands]
        self.assertTrue(any("--network tensorquay-staging_control" in name for name in names), names)

    def test_a_second_install_needs_its_own_network_pool(self):
        code, out, system = self.run_installer(self.yes("--name", "tensorquay-staging", "--port", "8081"))
        self.assertEqual(code, 2)
        self.assertIn("--network-pool", out.text)
        self.assertEqual(system.commands, [])

    def test_dry_run_writes_and_starts_nothing(self):
        code, out, system = self.run_installer(self.yes("--dry-run"))
        self.assertEqual(code, 0, out.text)
        self.assert_nothing_written()
        self.assertFalse(any(command[:2] != ["docker", "info"] and "stat" not in command
                             for command in system.commands), system.commands)
        for name in ("install.key", "secrets.toml", "models.toml", "gateway.toml", "compose.env"):
            self.assertIn(name, out.text)


class AfterTheStart(Case):
    """The probe's verdict, the https:// line, and what a failed step says."""

    def hosted(self, system, *more):
        key = private_file(self.home, "model-key", "the-key\n")
        return self.run_installer(
            ["--yes", "--model-url", "https://models.example/v1", "--model", "example-model", "--key-file", str(key),
             "--admin-email", "ana@example.org", "--admin-password-file", str(self.password_file), *more],
            system=system)

    def test_the_route_is_checked_once_and_its_verdict_printed(self):
        code, out, system = self.hosted(FakeSystem(probe=probe()))
        self.assertEqual(code, 0, out.text)
        call = next(request for request in system.requests if request[1].endswith("/probe"))
        self.assertEqual(call[0], "POST")
        self.assertTrue(call[3]["Idempotency-Key"])
        self.assertEqual(call[3]["Origin"], "http://localhost:8080")
        self.assertIn("Checking the model server… verified: it works for tasks (answered in 2 s).", out.lines)

    def test_lost_line_breaks_print_the_known_fix_and_change_nothing(self):
        failure = {"code": "strict_line_breaks_lost", "status": None, "excerpt": None}
        code, out, system = self.hosted(FakeSystem(probe=probe("failed", failure)))
        self.assertEqual(code, 0, out.text)
        index = out.lines.index("Checking the model server… the check failed (strict_line_breaks_lost).")
        self.assertEqual(out.lines[index + 1],
                         "  With strict tools the server dropped the line break from a tool's text. Fix: in "
                         "Settings › Models, set Strict tool schemas to Never on the route local, then check again.")
        self.assertEqual(sum(request[0] != "GET" and "/admin/routes" in request[1]
                             for request in system.requests), 1, "the route was changed")

    def test_another_failure_names_its_code_and_the_page(self):
        failure = {"code": "unreachable", "status": None, "excerpt": None}
        code, out, _ = self.hosted(FakeSystem(probe=probe("failed", failure)))
        self.assertEqual(code, 0)
        self.assertIn("Checking the model server… the check failed (unreachable).", out.lines)
        self.assertIn("  Settings › Models › Check names the reason and what to try.", out.lines)

    def test_a_check_past_a_minute_is_left_to_the_page(self):
        code, out, _ = self.hosted(FakeSystem(probe=TimeoutError("timed out")))
        self.assertEqual(code, 0)
        self.assertIn("Checking the model server… no verdict within a minute; run Check in Settings › Models.",
                      out.lines)

    def test_an_https_address_prints_the_proxy_line_and_nothing_else(self):
        code, out, system = self.hosted(FakeSystem(probe=probe()), "--address", "https://agent.example.org")
        self.assertEqual(code, 0, out.text)
        self.assertIn('public_origin="https://agent.example.org"\n', (self.install_dir / "gateway.toml").read_text())
        self.assertEqual(out.lines[-2], "Behind your TLS proxy, send https://agent.example.org to "
                                        "http://127.0.0.1:8080 and pass the Host and Origin headers through.")
        self.assertEqual(out.lines[-1], "Ready. Open https://agent.example.org and sign in as ana@example.org.")
        names = " ".join(" ".join(command) for command in system.commands)
        for word in ("cert", "caddy", "nginx", "traefik"):
            self.assertNotIn(word, names.lower())

    def test_a_failed_start_names_the_step_redacts_and_keeps_the_files(self):
        def stderr(env):
            compose = (self.install_dir / "compose.env").read_text()
            password = re.search(r"^TQA_POSTGRES_PASSWORD=(\w+)$", compose, re.M)[1]
            token = re.search(r"^TQA_BOOTSTRAP_TOKEN=(\w+)$", compose, re.M)[1]
            self.secrets = (password, token)
            return "postgres: password " + password + " refused\ntoken " + token + "\nadmin " + ADMIN_PASSWORD + "\n"

        code, out, _ = self.run_installer(self.yes(), system=FakeSystem(up_fails=stderr))
        self.assertEqual(code, 3)
        for secret in (*self.secrets, ADMIN_PASSWORD):
            self.assertNotIn(secret, out.text)
        self.assertIn("[REDACTED]", out.text)
        self.assertIn("Starting the stack failed", out.text)
        self.assertIn("The files stay in " + str(self.install_dir), out.text)
        self.assertTrue((self.install_dir / "compose.env").exists())

    def test_nothing_secret_is_printed_on_success(self):
        code, out, _ = self.run_installer(self.yes())
        self.assertEqual(code, 0)
        compose = (self.install_dir / "compose.env").read_text()
        for name in ("TQA_POSTGRES_PASSWORD", "TQA_BOOTSTRAP_TOKEN"):
            self.assertNotIn(re.search("^" + name + r"=(\w+)$", compose, re.M)[1], out.text)
        self.assertNotIn(ADMIN_PASSWORD, out.text)

    def test_a_refused_administrator_is_exit_4_with_the_way_on(self):
        code, out, _ = self.run_installer(self.yes(), system=FakeSystem(bootstrap=409))
        self.assertEqual(code, 4)
        self.assertIn("http://localhost:8080/setup", out.text)
        self.assertIn("TQA_BOOTSTRAP_TOKEN in " + str(self.install_dir / "compose.env"), out.text)

    def test_a_stack_never_ready_is_a_failed_step_after_three_minutes(self):
        code, out, system = self.run_installer(self.yes(), system=FakeSystem(ready_after=10_000))
        self.assertEqual(code, 3)
        self.assertGreaterEqual(system.slept, 180)
        self.assertLess(system.slept, 190)
        self.assertIn("didn't answer ready within 3 minutes", out.text)


class Redaction(unittest.TestCase):
    """deploy/tools/shell.py: the harness's redaction, shared with deploy/tests/compose.py."""

    def test_redact_removes_every_named_secret(self):
        env = {"TQA_POSTGRES_PASSWORD": "p" * 64, "TQA_BOOTSTRAP_TOKEN": "t" * 64, "TQA_STACK_PASSWORD": "s3cret!!"}
        text = shell.redact("a " + "p" * 64 + " b " + "t" * 64 + " c s3cret!!", env)
        self.assertEqual(text, "a [REDACTED] b [REDACTED] c [REDACTED]")
        self.assertEqual(shell.redact("x", {}), "x")

    def test_extra_values_are_redacted_too(self):
        self.assertEqual(shell.redact("admin hunter2hunter2", {}, extra=("hunter2hunter2",)), "admin [REDACTED]")

    def test_the_harness_uses_the_same_function(self):
        sys.path.insert(0, str(ROOT / "deploy" / "tests"))
        import compose  # noqa: E402,PLC0415

        self.assertIs(compose.redact, shell.redact)
        self.assertIs(compose.command_failure, shell.command_failure)

    def test_command_failure_names_the_command_and_its_last_lines_redacted(self):
        error = subprocess.CalledProcessError(1, ["docker"], stderr="\n".join("line %d" % n for n in range(60))
                                              + "\ntoken " + "t" * 64)
        stream = io.StringIO()
        shell.command_failure(["docker", "compose", "up"], {"TQA_BOOTSTRAP_TOKEN": "t" * 64}, error, file=stream)
        printed = stream.getvalue().splitlines()
        self.assertEqual(printed[0], "Command failed (exit 1): docker compose up")
        self.assertEqual(len(printed), 41)
        self.assertEqual(printed[-1], "token [REDACTED]")


class Wrapper(unittest.TestCase):
    """deploy/install.sh checks Python and the Docker versions, then hands over."""

    def test_install_sh_is_shellcheck_clean(self):
        if shutil.which("shellcheck") is None:
            self.skipTest("shellcheck isn't installed")
        done = subprocess.run(["shellcheck", str(ROOT / "deploy" / "install.sh")], capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stdout)

    def run_wrapper(self, engine, compose_version, *argv):
        bin_dir = Path(tempfile.mkdtemp(prefix="tqa-bin-"))
        self.addCleanup(shutil.rmtree, bin_dir, True)
        docker = bin_dir / "docker"
        docker.write_text("#!/bin/sh\nif [ \"$1\" = version ]; then echo " + engine + "; exit 0; fi\n"
                          "if [ \"$1 $2\" = 'compose version' ]; then echo " + compose_version + "; exit 0; fi\n"
                          "exit 9\n")
        docker.chmod(0o755)
        env = {"PATH": str(bin_dir) + ":/usr/bin:/bin", "HOME": tempfile.gettempdir()}
        return subprocess.run(["bash", str(ROOT / "deploy" / "install.sh"), *argv], cwd=ROOT, env=env,
                              capture_output=True, text=True, timeout=60)

    def test_old_docker_is_refused_before_anything(self):
        done = self.run_wrapper("26.1.0", "2.31.0", "--dry-run")
        self.assertEqual(done.returncode, 2)
        self.assertIn("Docker Engine 27.4.0 or newer and Docker Compose 2.31.0 or newer are required", done.stderr)
        done = self.run_wrapper("27.4.0", "2.30.9", "--dry-run")
        self.assertEqual(done.returncode, 2)

    def test_help_is_the_installer_s_own(self):
        done = self.run_wrapper("28.0.1", "2.35.1", "--help")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("--fake-model", done.stdout)


if __name__ == "__main__":
    unittest.main()
