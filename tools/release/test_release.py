"""REL-U-007: the local release pipeline's orchestration over a recording runner (no Docker, no repository)."""

import json
from pathlib import Path
import subprocess
import tempfile
import unittest

import release
import release_checks
from test_release_manifest import FILES

SHA = "a" * 40
PAGE = "# Gate 7: Channels\n\n**State:** the gate is **reached**, by the evaluation.\n"


class Fake:
    """Plays git, Docker and the release tools; records every command."""

    def __init__(self, porcelain="", on_main=True, tag="", revision_label="8", fail_on=None):
        self.calls, self.porcelain, self.on_main, self.tag = [], porcelain, on_main, tag
        self.revision_label, self.fail_on, self.built = revision_label, fail_on, 0

    def __call__(self, arguments, cwd=None, env=None, timeout=None):
        self.calls.append(list(arguments))
        text = " ".join(str(argument) for argument in arguments)
        if self.fail_on and self.fail_on in text:
            raise subprocess.CalledProcessError(1, arguments, output="", stderr="injected failure")
        if arguments[:1] == ["which"]:
            return "/usr/bin/" + arguments[1]
        if arguments[0] == "git":
            return self.git(arguments)
        if arguments[:2] == ["tar", "-x"]:
            export = Path(arguments[arguments.index("-C") + 1])
            for relative, content in dict(FILES, **{"docs/evals/gate-7.md": PAGE}).items():
                path = export / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content)
            return ""
        if arguments[:2] == ["docker", "build"]:
            if "--output" in arguments:
                kernel = Path(arguments[arguments.index("--output") + 1].split("dest=")[1])
                kernel.mkdir(parents=True)
                (kernel / "manifest.json").write_text(json.dumps({"vmlinux_sha256": "9" * 64}))
                (kernel / "config").write_text("CONFIG_X=y\n")
                return ""
            self.built += 1
            return "sha256:" + str(self.built) * 64
        if arguments[:3] == ["docker", "image", "inspect"]:
            return json.dumps([{"Size": 1000, "Config": {"Labels": {
                "org.tensorquay.protocol-revision": self.revision_label}}}])
        if "build_rootfs.py" in text:
            image = arguments[arguments.index("--image") + 1].removeprefix("sha256:")
            directory = Path(arguments[arguments.index("--out") + 1]) / image
            directory.mkdir(parents=True)
            (directory / "manifest.json").write_text(json.dumps({"rootfs": {"sha256": "r" + image[1:]},
                                                                 "source_image": {"id": "sha256:" + image}}))
            return ""
        if "sbom.py" in text:
            out = Path(arguments[arguments.index("--out") + 1])
            out.mkdir(parents=True, exist_ok=True)
            name = arguments[arguments.index("--artefact") + 1]
            (out / (name + ".spdx.json")).write_text("{}")
            return ""
        if "sbom_vulns.py" in text:
            sbom = Path(arguments[arguments.index("--sbom") + 1])
            (sbom / "vulns.json").write_text(json.dumps({"passed": True, "counts": {"web": {"OPEN": 2}}}))
            (sbom / "vulns.md").write_text("# gate\n")
            return ""
        raise AssertionError("unexpected command: " + text)

    def git(self, arguments):
        if "status" in arguments:
            return self.porcelain
        if "merge-base" in arguments:
            if not self.on_main:
                raise subprocess.CalledProcessError(1, arguments)
            return ""
        if "refs/tags/" in " ".join(arguments):
            return self.tag
        if "rev-parse" in arguments:
            return "c" * 40 + "\n" if "HEAD" in arguments else SHA + "\n"
        if "show" in arguments:
            return "1790540000\n" if "--format=%ct" in arguments else "2026-09-28T05:00:00+08:00\n"
        if "archive" in arguments:
            return ""
        raise AssertionError("unexpected git: " + " ".join(arguments))


class Pipeline(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.out = Path(directory.name) / "dist"
        self.out.mkdir()

    def pipeline(self, runner, **options):
        settings = dict(version="1.0.0", commit="main", candidate=False, kernel=False, tracker=None)
        settings.update(options)
        return release.Pipeline(repo=Path("/repo"), out=self.out, run=runner, env={"TQA_DOCKER_TESTS": "1"},
                                **settings)

    def test_a_release_builds_everything_and_writes_its_directory(self):
        fake = Fake()
        final = self.pipeline(fake, kernel=True).run()
        self.assertEqual(final, self.out / "1.0.0")
        record = json.loads((final / "release.json").read_text())
        self.assertEqual(set(record["images"]), {"gateway", "web", "runtime-base", "runtime-workstation", "runtime-desktop", "vmm"})
        self.assertEqual(set(record["rootfs"]), {"base", "workstation", "desktop"})
        self.assertEqual(record["kernel"]["sha256"], "9" * 64)
        self.assertTrue(record["releasable"])
        self.assertEqual(record["tag"], {"tag": "v1.0.0", "state": "to create"})
        for name in ("RELEASE-NOTES.md", "SHA256SUMS", "sbom/vulns.md", "rootfs/base.manifest.json",
                     "kernel/manifest.json", "kernel/config"):
            self.assertTrue((final / name).is_file(), name)
        self.assertIn("  release.json", (final / "SHA256SUMS").read_text())
        self.assertFalse(any(self.out.glob(".*")), "no partial or work directory is left")

    def test_the_steps_run_in_order_from_the_export(self):
        fake = Fake()
        self.pipeline(fake).run()
        kinds = [release.step_of(call) for call in fake.calls if release.step_of(call)]
        order = [kinds.index(step) for step in ("export", "build", "rootfs", "sbom", "vulnerabilities")]
        self.assertEqual(order, sorted(order))
        builds = [call for call in fake.calls if call[:2] == ["docker", "build"]]
        self.assertTrue(all("SOURCE_DATE_EPOCH=1790540000" in call for call in builds))

    def test_nothing_is_ever_published_or_tagged(self):
        fake = Fake()
        self.pipeline(fake, kernel=True).run()
        for call in fake.calls:
            self.assertNotIn(tuple(call[:2]), release.FORBIDDEN)

    def test_the_guard_refuses_a_publishing_command_outright(self):
        guarded = release.guard(Fake())
        for command in (["docker", "push", "x"], ["git", "tag", "v1"], ["git", "push"], ["docker", "login"]):
            with self.subTest(command), self.assertRaises(release_checks.Refused):
                guarded(command)

    def test_a_dirty_checkout_is_refused_before_anything_is_built(self):
        fake = Fake(porcelain=" M tools/release/release.py\n")
        with self.assertRaises(release_checks.Refused):
            self.pipeline(fake).run()
        self.assertFalse(any(call[:2] == ["docker", "build"] for call in fake.calls))
        self.assertFalse(any(self.out.iterdir()))

    def test_a_moved_tag_is_refused_before_anything_is_built(self):
        fake = Fake(tag="b" * 40)
        with self.assertRaisesRegex(release_checks.Refused, "moved"):
            self.pipeline(fake).run()
        self.assertFalse(any(call[:2] == ["docker", "build"] for call in fake.calls))

    def test_a_commit_off_main_needs_candidate_and_is_then_not_releasable(self):
        with self.assertRaisesRegex(release_checks.Refused, "main"):
            self.pipeline(Fake(on_main=False)).run()
        final = self.pipeline(Fake(on_main=False), candidate=True, version="0.9.0-rc.1").run()
        self.assertFalse(json.loads((final / "release.json").read_text())["releasable"])
        self.assertIn("not releasable", (final / "RELEASE-NOTES.md").read_text())

    def test_a_failed_gate_leaves_only_the_partial_directory_with_its_reason(self):
        fake = Fake(fail_on="sbom_vulns.py")
        with self.assertRaises(release.Failed):
            self.pipeline(fake).run()
        self.assertFalse((self.out / "1.0.0").exists())
        failed = (self.out / ".1.0.0.partial" / "FAILED").read_text()
        self.assertIn("vulnerabilities", failed)
        self.assertIn("injected failure", failed)

    def test_a_runtime_image_speaking_another_protocol_revision_fails(self):
        with self.assertRaisesRegex(release.Failed, "protocol revision"):
            self.pipeline(Fake(revision_label="7")).run()


if __name__ == "__main__":
    unittest.main()
