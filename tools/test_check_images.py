"""SEC-030 (G-05-images): every third-party image a compose file names is on the approved list, pinned by the digest
the list records, under an Apache-2.0-compatible licence (ADR-0017 point 8). Our own images need no entry."""

import importlib.util
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parent / "check-images.py"
SPEC = importlib.util.spec_from_file_location("check_images", SOURCE)
check_images = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_images)

PG = "sha256:" + "a" * 64
CADDY = "sha256:" + "b" * 64
LIST = f"""
[[image]]
repository = "postgres"
digest = "{PG}"
licence = "PostgreSQL"
source = "https://www.postgresql.org/about/licence/"

[[image]]
repository = "caddy"
digest = "{CADDY}"
licence = "Apache-2.0"
source = "https://github.com/caddyserver/caddy/blob/master/LICENSE"
"""
COMPOSE = f"""services:
  postgres:
    image: postgres:18.3-alpine@{PG}
  gateway:
    image: ${{TQA_GATEWAY_IMAGE:-tqa-gateway:local}}
  proxy:
    image: "caddy:2.10.2@{CADDY}"
"""


def run(compose=COMPOSE, listing=LIST, extra=None):
    """Checks a temporary tree; returns the list of problems."""
    with tempfile.TemporaryDirectory() as root:
        compose_dir = Path(root, "deploy", "compose")
        compose_dir.mkdir(parents=True)
        Path(compose_dir, "docker.yml").write_text(compose)
        for name, text in (extra or {}).items():
            Path(compose_dir, name).write_text(text)
        Path(root, "deploy", "third-party-images.toml").write_text(listing)
        return check_images.check_tree(Path(root))


class ApprovedImages(unittest.TestCase):
    def test_listed_pinned_images_and_our_own_pass(self):
        self.assertEqual(run(), [])

    def test_an_image_off_the_list_fails_naming_the_file_and_image(self):
        problems = run(extra={"extra.yml": f"services:\n  cache:\n    image: redis:8@sha256:{'c' * 64}\n"})
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("extra.yml", problems[0])
        self.assertIn("redis", problems[0])
        self.assertIn("not on the approved list", problems[0])

    def test_an_unpinned_or_differently_pinned_image_fails(self):
        for image in ("postgres:18.3-alpine", f"postgres:18.3-alpine@sha256:{'d' * 64}", "postgres@sha256:short"):
            with self.subTest(image=image):
                problems = run(compose=COMPOSE.replace(f"postgres:18.3-alpine@{PG}", image))
                self.assertTrue(any("digest" in problem for problem in problems), problems)

    def test_a_listed_licence_must_be_apache_compatible_and_never_source_available(self):
        for licence in ("BUSL-1.1", "SSPL-1.0", "AGPL-3.0", "GPL-2.0", "LGPL-3.0", "Elastic-2.0",
                        "LicenseRef-Infisical-Enterprise", "proprietary", "see licence in file", ""):
            with self.subTest(licence=licence):
                problems = run(listing=LIST.replace('"PostgreSQL"', f'"{licence}"'))
                self.assertTrue(any("licence" in problem for problem in problems), problems)
        for licence in ("MIT", "BSD-3-Clause", "MPL-2.0", "0BSD", "ISC", "PostgreSQL", "Apache-2.0"):
            with self.subTest(licence=licence):
                self.assertEqual(run(listing=LIST.replace('"PostgreSQL"', f'"{licence}"')), [])

    def test_docker_hub_spellings_name_the_same_repository(self):
        for spelling in ("docker.io/library/postgres", "docker.io/postgres", "library/postgres"):
            with self.subTest(spelling=spelling):
                compose = COMPOSE.replace(f"image: postgres:18.3-alpine@{PG}", f"image: {spelling}:18@{PG}")
                self.assertEqual(run(compose=compose), [])

    def test_a_substituted_image_is_checked_by_its_default_and_an_unknown_one_fails(self):
        compose = COMPOSE.replace(f"image: postgres:18.3-alpine@{PG}", f"image: ${{DB_IMAGE:-postgres:18@{PG}}}")
        self.assertEqual(run(compose=compose), [])
        compose = COMPOSE.replace(f"image: postgres:18.3-alpine@{PG}", "image: ${DB_IMAGE}")
        problems = run(compose=compose)
        self.assertTrue(any("cannot be checked" in problem for problem in problems), problems)

    def test_our_own_images_need_the_tqa_prefix(self):
        compose = COMPOSE.replace("tqa-gateway:local", "someone-else:local")
        problems = run(compose=compose)
        self.assertTrue(any("someone-else" in problem for problem in problems), problems)

    def test_only_the_exact_tqa_prefix_marks_our_own_images(self):
        for image in ("tquay-thing:local", "tqa:local", "tqagateway:local", "other/tqa-gateway:local"):
            with self.subTest(image=image):
                problems = run(compose=COMPOSE.replace("tqa-gateway:local", image))
                self.assertTrue(any("not on the approved list" in problem for problem in problems), problems)

    def test_an_exact_duplicate_entry_is_reported(self):
        duplicate = LIST + f"""
[[image]]
repository = "postgres"
digest = "{PG}"
licence = "PostgreSQL"
source = "https://www.postgresql.org/about/licence/"
"""
        problems = run(listing=duplicate)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("listed twice", problems[0])

    def test_an_unused_entry_is_reported_so_the_list_stays_current(self):
        problems = run(compose=COMPOSE.replace(f'  proxy:\n    image: "caddy:2.10.2@{CADDY}"\n', ""))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("caddy", problems[0])
        self.assertIn("no compose file uses", problems[0])

    def test_a_malformed_list_fails_closed(self):
        for listing in ("not toml [", '[[image]]\nrepository = "postgres"\n', 'owner = "someone"\n' + LIST,
                        LIST + '\n[[image]]\nrepository = "postgres"\ndigest = "x"\nlicence = "MIT"\nsource = "s"\n',
                        LIST.replace('source = "https://www.postgresql.org/about/licence/"',
                                     'source = "s"\nextra = true')):
            with self.subTest(listing=listing[:40]):
                self.assertTrue(run(listing=listing), "a malformed list must fail")

    def test_the_repository_list_itself_passes(self):
        root = Path(__file__).resolve().parents[1]
        self.assertEqual(check_images.check_tree(root), [])


if __name__ == "__main__":
    unittest.main()


# The images' architecture decisions (the review's B1 and B3): each Dockerfile makes its one choice in a small
# 'arch-check' stage, with no download and no package install, and its real stages COPY that choice, so nothing
# drifts from what these tests run. The stage's RUN text is executed here as Docker would (sh -c), with TARGETARCH
# set, in every check.sh run and without Docker.
ROOT = Path(__file__).resolve().parents[1]


def arch_check(dockerfile):
    """The 'arch-check' stage of `dockerfile`: its ARG defaults and its RUN commands, continuations joined."""
    text = re.sub(r"\\\n\s*", " ", (ROOT / dockerfile).read_text())
    stage, args, runs = False, {}, []
    for line in text.splitlines():
        if line.startswith("FROM "):
            stage = line.rstrip().endswith(" AS arch-check")
        elif stage and line.startswith("ARG "):
            name, _, default = line[4:].strip().partition("=")
            args[name] = default
        elif stage and line.startswith("RUN "):
            runs.append(line[4:].strip())
    return args, runs


def run_arch_check(dockerfile, arch):
    """Runs the stage for `arch`: (exit code, stderr, the files it wrote)."""
    args, runs = arch_check(dockerfile)
    assert runs, "no arch-check stage in " + dockerfile
    env = {"PATH": "/usr/bin:/bin", **{k: v for k, v in args.items() if v}, "TARGETARCH": arch}
    with tempfile.TemporaryDirectory() as work:
        for command in runs:
            done = subprocess.run(["sh", "-c", command], cwd=work, env=env, capture_output=True, text=True, check=False)
            if done.returncode != 0:
                return done.returncode, done.stderr, {}
        return 0, "", {path.name: path.read_text() for path in Path(work).iterdir()}


class ArchitectureChecks(unittest.TestCase):
    RUNTIME, VMM, KERNEL = "images/runtime/Dockerfile", "images/vmm/Dockerfile", "images/rootfs/kernel/Dockerfile"

    def test_the_runtime_takes_go_for_amd64_and_arm64_with_each_ones_checksum(self):
        args, _ = arch_check(self.RUNTIME)
        for arch, key in (("amd64", "GO_SHA256_AMD64"), ("arm64", "GO_SHA256_ARM64")):
            with self.subTest(arch=arch):
                code, _, wrote = run_arch_check(self.RUNTIME, arch)
                self.assertEqual(code, 0)
                self.assertEqual(wrote["go-url"].strip(),
                                 "https://go.dev/dl/go{}.linux-{}.tar.gz".format(args["GO_VERSION"], arch))
                self.assertEqual(wrote["go-sha256"].split()[0], args[key])
                self.assertRegex(args[key], r"^[0-9a-f]{64}$")

    def test_the_runtime_refuses_another_architecture_in_plain_words_before_any_download(self):
        code, stderr, wrote = run_arch_check(self.RUNTIME, "riscv64")
        self.assertNotEqual(code, 0)
        self.assertIn("the Go toolchain is pinned for amd64 and arm64 only, not riscv64", stderr)
        self.assertEqual(wrote, {})

    def test_the_microvm_images_refuse_arm64_in_plain_words_and_take_amd64(self):
        for dockerfile, words in ((self.VMM, "The microVM backend (Firecracker) is amd64-only; on arm64 use the Docker "
                                             "backend"),
                                  (self.KERNEL, "The microVM guest kernel is built for amd64 only; on arm64 use the "
                                                "Docker backend")):
            with self.subTest(dockerfile=dockerfile):
                code, stderr, _ = run_arch_check(dockerfile, "arm64")
                self.assertNotEqual(code, 0)
                self.assertIn(words, stderr)
                self.assertEqual(run_arch_check(dockerfile, "amd64")[0], 0)

    def test_the_real_stages_copy_their_choice_from_arch_check(self):
        # Nothing may decide the architecture a second time: the stage that downloads or builds takes arch-check's files.
        for dockerfile in (self.RUNTIME, self.VMM, self.KERNEL):
            with self.subTest(dockerfile=dockerfile):
                text = (ROOT / dockerfile).read_text()
                self.assertTrue("COPY --from=arch-check" in text, dockerfile + " has no COPY --from=arch-check")
                code = "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("#"))
                self.assertNotRegex(re.sub(r"(?s)FROM [^\n]* AS arch-check\n.*?(?=\nFROM )", "", code),
                                    r"TARGETARCH", dockerfile + ": only arch-check may read TARGETARCH")


def stage_labels(dockerfile, stage):
    """The LABEL key/value pairs set in `stage` itself (not inherited), continuations joined."""
    text = re.sub(r"\\\n\s*", " ", (ROOT / dockerfile).read_text())
    inside, labels = False, {}
    for line in text.splitlines():
        if line.startswith("FROM "):
            inside = line.rstrip().endswith(" AS " + stage)
        elif inside and line.startswith("LABEL "):
            labels.update(re.findall(r'([\w.-]+)="([^"]*)"', line))
    return labels


def stage_text(dockerfile, stage):
    """The lines of `stage` itself (continuations joined), up to the next FROM."""
    text = re.sub(r"\\\n\s*", " ", (ROOT / dockerfile).read_text())
    inside, lines = False, []
    for line in text.splitlines():
        if line.startswith("FROM "):
            inside = line.rstrip().endswith(" AS " + stage)
        elif inside:
            lines.append(line)
    return "\n".join(lines)


def stage_chain(dockerfile, stage):
    """`stage` and the stages it builds on, nearest first, up to the first that starts from an image."""
    text = (ROOT / dockerfile).read_text()
    parents = dict((name, parent) for parent, name in re.findall(r"(?m)^FROM (?:--\S+ )*(\S+) AS ([\w-]+)", text))
    chain = []
    while stage in parents:
        chain.append(stage)
        stage = parents[stage]
    return chain


def chain_text(dockerfile, stage):
    """The lines a target's image is built from: its stage and every stage it builds on, oldest first."""
    return "\n".join(stage_text(dockerfile, name) for name in reversed(stage_chain(dockerfile, stage)))


class EnvironmentLabels(unittest.TestCase):
    # The built-in environments (design-environments): each runtime target says which environment it is, its name and
    # the capabilities it really has, so the gateway can register it with nobody typing an image ID.
    RUNTIME = "images/runtime/Dockerfile"
    PREFIX = "org.tensorquay.environment"

    def test_minimal_and_workstation_say_what_they_are(self):
        self.assertEqual({k: v for k, v in stage_labels(self.RUNTIME, "base").items() if k.startswith(self.PREFIX)},
                         {self.PREFIX: "minimal", self.PREFIX + ".name": "Minimal",
                          self.PREFIX + ".capabilities": "shell,python"})
        self.assertEqual(
            {k: v for k, v in stage_labels(self.RUNTIME, "workstation").items() if k.startswith(self.PREFIX)},
            {self.PREFIX: "workstation", self.PREFIX + ".name": "Workstation",
             self.PREFIX + ".capabilities": "shell,python,node,build,browser,data"})

    def test_data_is_claimed_only_where_the_python_data_libraries_are_installed(self):
        for stage in ("base", "workstation"):
            claims = stage_labels(self.RUNTIME, stage).get(self.PREFIX + ".capabilities", "").split(",")
            installed = all(package in chain_text(self.RUNTIME, stage)
                            for package in ("python3-pandas", "python3-matplotlib", "python3-openpyxl"))
            self.assertEqual("data" in claims, installed, stage)

    def test_browser_is_claimed_only_where_chromium_is_installed(self):
        for stage in ("base", "workstation"):
            claims = stage_labels(self.RUNTIME, stage).get(self.PREFIX + ".capabilities", "").split(",")
            self.assertEqual("browser" in claims, "chromium" in chain_text(self.RUNTIME, stage), stage)

    # GW-U-101 (the first hour's F3): python3 and its python link come with the runtime base, so every built-in on it
    # says Python; Minimal said Shell alone, and its card and its picker disagreed.
    def test_python_is_claimed_by_every_built_in_on_the_runtime_base_that_has_it(self):
        self.assertRegex(stage_text(self.RUNTIME, "runtime-base"), r"apt-get [^&]*\binstall\b[^&]*\bpython3\b")
        for stage in ("base", "workstation"):
            with self.subTest(stage=stage):
                self.assertIn("runtime-base", stage_chain(self.RUNTIME, stage))
                claims = stage_labels(self.RUNTIME, stage).get(self.PREFIX + ".capabilities", "").split(",")
                self.assertIn("python", claims)

    def test_test_images_built_from_the_runtime_base_carry_no_environment_label(self):
        for stage in ("runtime-base", "clone-probe", "go-toolchain"):
            with self.subTest(stage=stage):
                self.assertFalse(any(k.startswith(self.PREFIX) for k in stage_labels(self.RUNTIME, stage)))


class PythonName(unittest.TestCase):
    """RT-I-035's static half: the runtime base links `python` to `python3` (no package: Debian's python-is-python3 is
    GPL-3.0), so Minimal and Workstation, which both build on it, answer to either name."""

    def test_the_runtime_base_links_python_to_python3(self):
        text = stage_text("images/runtime/Dockerfile", "runtime-base")
        self.assertRegex(text, r"ln -s[f]? (/usr/bin/)?python3 /usr/(local/)?bin/python\b")
        installs = re.sub(r"\\\n\s*", " ", (ROOT / "images/runtime/Dockerfile").read_text())
        self.assertNotRegex(installs, r"apt-get install[^\n]*python-is-python3", "never the GPL-3.0 package")


class CanonicalTags(unittest.TestCase):
    # A built-in environment is an image with the environment label AND its canonical tag, both agreeing
    # (design-environments): an image built FROM Workstation inherits the label but never gets the tag.
    def test_build_images_gives_each_environment_target_its_canonical_tag(self):
        script = re.sub(r"\\\n\s*", " ", (ROOT / "deploy/build-images.sh").read_text())
        for target in ("base", "workstation"):
            with self.subTest(target=target):
                environment = stage_labels("images/runtime/Dockerfile", target)["org.tensorquay.environment"]
                builds = [line for line in script.splitlines()
                          if "--target " + target + " " in line and "images/runtime/Dockerfile" in line]
                self.assertEqual(len(builds), 1, "one build of the {} target".format(target))
                self.assertIn("-t tqa-runtime:" + environment, builds[0])


class DebianSnapshot(unittest.TestCase):
    """REL-U-010: the runtime base's apt reads the signed Debian snapshot the other stages read, so a cached OS layer is
    an exact, reviewed package set (it read the image's live mirror)."""
    RUNTIME = "images/runtime/Dockerfile"

    def test_the_runtime_base_installs_from_the_snapshot(self):
        text = stage_text(self.RUNTIME, "runtime-base")
        update = text.find("apt-get ")
        self.assertGreater(update, -1, "the runtime base installs its packages with apt")
        for archive in ("debian", "debian-security"):
            with self.subTest(archive=archive):
                # http, as the live source was: the slim image has no CA certificates yet; apt checks the signed
                # Release file and every package's hash either way.
                self.assertRegex(text[:update],
                                 r"https?://snapshot\.debian\.org/archive/{}/\d{{8}}T\d{{6}}Z/ trixie".format(archive))
        self.assertIn("rm /etc/apt/sources.list.d/debian.sources", text[:update], "no live source left")
        self.assertRegex(text, r"apt-get -o APT::Update::Error-Mode=any[^&]* update")

    def test_every_stage_reads_one_snapshot(self):
        text = (ROOT / self.RUNTIME).read_text()
        dates = set(re.findall(r"snapshot\.debian\.org/archive/debian(?:-security)?/(\d{8}T\d{6}Z)/", text))
        self.assertEqual(len(dates), 1, dates)

    # The security archive's snapshot that first holds every fix the c17 release dry run's vulnerability gate asked
    # for (chromium 156.0.8078.73-1~deb13u1, libheif 1.23.6-2~deb13u1, brotli 1.1.0-2+deb13u1, ca-certificates
    # 20250419+deb13u1; its Release file is dated 29 Oct 2026 07:24 UTC). REL-I-003 checks the versions built.
    AUDITED = "20261029T072456Z"

    def test_the_snapshot_is_at_least_the_audited_security_update(self):
        text = (ROOT / self.RUNTIME).read_text()
        dates = set(re.findall(r"snapshot\.debian\.org/archive/debian(?:-security)?/(\d{8}T\d{6}Z)/", text))
        self.assertTrue(dates and all(date >= self.AUDITED for date in dates), dates)


class BinaryLast(unittest.TestCase):
    """REL-U-011: the runtime binary is each shipped target's last filesystem change, so a change to the program alone
    reuses every OS, toolchain and package layer below it (it was copied into runtime-base, under all of them)."""
    RUNTIME = "images/runtime/Dockerfile"
    BINARY = "COPY --from=build /build/target/release/tqa-runtime /usr/local/bin/tqa-runtime"
    CHANGES = ("RUN ", "COPY ", "ADD ", "WORKDIR ")

    def changes(self, stage):
        return [line.strip() for line in stage_text(self.RUNTIME, stage).splitlines()
                if line.strip().startswith(self.CHANGES)]

    def test_each_shipped_target_copies_the_runtime_last(self):
        for target in ("base", "workstation", "desktop"):
            with self.subTest(target=target):
                self.assertEqual(self.changes(target)[-1:], [self.BINARY])

    def test_no_stage_a_shipped_target_builds_on_holds_the_runtime(self):
        for target in ("base", "workstation", "desktop"):
            for stage in stage_chain(self.RUNTIME, target)[1:]:
                with self.subTest(target=target, stage=stage):
                    self.assertNotIn(self.BINARY, self.changes(stage))

    def test_the_stage_chain_follows_from_to_the_image(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "Dockerfile"
            path.write_text("FROM debian@sha256:x AS os\nRUN true\nFROM --platform=$BUILDPLATFORM os AS mid\n"
                            "FROM mid AS top\nCOPY a b\n")
            self.assertEqual(stage_chain(path, "top"), ["top", "mid", "os"])
            self.assertEqual(chain_text(path, "top"), "RUN true\n\nCOPY a b")
