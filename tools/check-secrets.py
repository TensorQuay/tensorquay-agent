#!/usr/bin/env python3
"""G-10: scan tracked/new nonignored working files and all reachable history with pinned Gitleaks."""

import argparse
from contextlib import redirect_stdout
import io
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch


GITLEAKS_VERSION = "8.30.1"
SCAN_TIMEOUT_SECONDS = 300
INSTALL_COMMAND = './tools/install-gitleaks.sh && export PATH="$HOME/.local/bin:$PATH"'


def require_gitleaks():
    """Fail with an actionable install command when the pinned scanner is unavailable."""
    if not shutil.which("gitleaks"):
        raise ValueError("gitleaks {} is required; install: {}".format(GITLEAKS_VERSION, INSTALL_COMMAND))
    result = subprocess.run(["gitleaks", "version"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            encoding="utf-8", timeout=30, check=False)
    if result.returncode or result.stdout.strip() != GITLEAKS_VERSION:
        raise ValueError("gitleaks {} is required; install: {}".format(GITLEAKS_VERSION, INSTALL_COMMAND))


def require_full_history(root):
    """Reject shallow checkouts rather than treating a partial history scan as complete."""
    result = subprocess.run(["git", "rev-parse", "--is-shallow-repository"], cwd=root,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            encoding="utf-8", timeout=30, check=False)
    if result.returncode or result.stdout.strip() != "false":
        raise ValueError("G-10 requires full history; fetch it with: git fetch --unshallow --tags")


def scan(mode, path, config, cwd=None):
    """Run the real scanner with full redaction and only the reviewed configuration."""
    command = ["gitleaks", mode, str(path), "--config", str(config), "--redact=100",
               "--no-banner", "--no-color", "--verbose", "--ignore-gitleaks-allow",
               "--gitleaks-ignore-path", os.devnull, "--timeout=" + str(SCAN_TIMEOUT_SECONDS)]
    if mode == "git":
        command.append("--log-opts=--all")
    return subprocess.run(command, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                          encoding="utf-8", timeout=SCAN_TIMEOUT_SECONDS + 30, check=False)


def working_tree_paths(root):
    """Select tracked files, even when ignored, plus new files Git does not ignore."""
    result = subprocess.run(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
                            cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            encoding="utf-8", timeout=30, check=True)
    return sorted(set(filter(None, result.stdout.split("\0"))))


def snapshot_files(root, snapshot, names):
    """Copy current working bytes without following links outside the selected subtree."""
    for name in names:
        source = root / name
        if not source.exists() and not source.is_symlink():
            continue  # A tracked file deleted locally has no working-tree bytes to scan.
        resolved = source.resolve(strict=True)
        try:
            resolved.relative_to(root.resolve())
        except ValueError:
            raise ValueError(name + " is a symlink that points outside the repository") from None
        if not source.is_file():
            raise ValueError("cannot snapshot a non-file: " + name)
        target = snapshot / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(str(source), str(target))


def scan_working_tree(root, config):
    """Scan an isolated copy of publishable files, preserving paths for fixture exceptions."""
    with tempfile.TemporaryDirectory() as directory:
        snapshot = Path(directory)
        snapshot_files(root, snapshot, working_tree_paths(root))
        return scan("dir", Path("."), config, cwd=snapshot)


def run_check(root):
    """Scan selected working files and all history, preserving either failure."""
    try:
        require_gitleaks()
        require_full_history(root)
        failed = False
        for mode, label in (("dir", "working tree"), ("git", "full history")):
            config = root / ".gitleaks.toml"
            result = scan_working_tree(root, config) if mode == "dir" else scan(mode, Path("."), config, cwd=root)
            print(result.stdout, end="", flush=True)
            print("G-10: {} {}".format(label, "failed" if result.returncode else "clean"), flush=True)
            failed = failed or result.returncode != 0
        return 1 if failed else 0
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print("G-10: " + str(error))
        return 1


class SecretsTests(unittest.TestCase):
    """Use synthetic values, generated only in temporary files, to test the real scanner."""

    def test_synthetic_key_shapes_outside_and_inside_tree_are_redacted(self):
        samples = [
            "sk-" + "0123456789abcdef" * 2,
            "ghp_" + "A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8",
            "-----BEGIN " + "PRIVATE KEY-----",
        ]
        require_gitleaks()
        root = Path(__file__).resolve().parent.parent
        with tempfile.TemporaryDirectory() as outside:
            for parent in (Path(outside), root):
                for sample in samples:
                    with self.subTest(location=parent == root, shape=sample[:3]):
                        with tempfile.NamedTemporaryFile(dir=parent, suffix=".txt") as fixture:
                            fixture.write((sample + "\n").encode())
                            fixture.flush()
                            result = scan("dir", Path(fixture.name), root / ".gitleaks.toml")
                        self.assertEqual(result.returncode, 1, result.stdout)
                        self.assertNotIn(sample, result.stdout)
                        self.assertIn("REDACTED", result.stdout)

    def test_clean_directory_passes(self):
        require_gitleaks()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "clean.txt").write_text("No credentials here.\n", encoding="utf-8")
            result = scan("dir", root, Path(__file__).resolve().parent.parent / ".gitleaks.toml")
            self.assertEqual(result.returncode, 0, result.stdout)

    def test_inline_suppression_cannot_hide_a_synthetic_key(self):
        require_gitleaks()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake = "sk-" + "0123456789abcdef" * 2
            (root / "value.txt").write_text(fake + " # gitleaks:allow\n", encoding="utf-8")
            result = scan("dir", root, Path(__file__).resolve().parent.parent / ".gitleaks.toml")
            self.assertEqual(result.returncode, 1, result.stdout)
            self.assertNotIn(fake, result.stdout)

    def test_missing_binary_fails_with_the_install_command(self):
        with patch("shutil.which", return_value=None):
            with self.assertRaisesRegex(ValueError, r"tools/install-gitleaks.sh"):
                require_gitleaks()

    def test_different_version_is_rejected(self):
        with patch("shutil.which", return_value="gitleaks"), patch("subprocess.run") as run:
            run.return_value.stdout = "8.0.0\n"
            run.return_value.returncode = 0
            with self.assertRaisesRegex(ValueError, "8.30.1"):
                require_gitleaks()

    def test_history_includes_every_ref_and_disables_inline_suppression(self):
        with patch("subprocess.run") as run:
            scan("git", Path("."), Path(".gitleaks.toml"))
            command = run.call_args[0][0]
            for argument in ("--log-opts=--all", "--redact=100", "--ignore-gitleaks-allow"):
                self.assertIn(argument, command)

    def test_shallow_repository_fails_closed(self):
        with patch("subprocess.run") as run:
            run.return_value.stdout = "true\n"
            run.return_value.returncode = 0
            with self.assertRaisesRegex(ValueError, "full history"):
                require_full_history(Path("."))

    def test_snapshot_keeps_current_tracked_and_new_bytes_and_omits_unselected_build_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "repo"
            snapshot = Path(directory) / "snapshot"
            root.mkdir()
            snapshot.mkdir()
            (root / "tracked.txt").write_text("current edited bytes", encoding="utf-8")
            (root / "new.txt").write_text("new file bytes", encoding="utf-8")
            (root / "target").mkdir()
            (root / "target/output").write_text("build output", encoding="utf-8")
            snapshot_files(root, snapshot, ["tracked.txt", "new.txt", "deleted.txt"])
            self.assertEqual((snapshot / "tracked.txt").read_text(), "current edited bytes")
            self.assertEqual((snapshot / "new.txt").read_text(), "new file bytes")
            self.assertFalse((snapshot / "target").exists())
            self.assertFalse((snapshot / "deleted.txt").exists())

    def test_snapshot_rejects_external_or_dangling_symlinks(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "repo"
            snapshot = Path(directory) / "snapshot"
            root.mkdir()
            snapshot.mkdir()
            (root.parent / "outside.txt").write_text("outside", encoding="utf-8")
            for target in ("../outside.txt", "missing.txt"):
                link = root / "link.txt"
                link.symlink_to(target)
                with self.subTest(target=target), self.assertRaises((ValueError, OSError)):
                    snapshot_files(root, snapshot, ["link.txt"])
                link.unlink()

    def test_external_symlink_error_names_only_the_repository_relative_path(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "repo"
            (root / "links").mkdir(parents=True)
            (root.parent / "outside.txt").write_text("outside", encoding="utf-8")
            (root / "links/escape.txt").symlink_to("../../outside.txt")
            output = io.StringIO()
            with patch(__name__ + ".require_gitleaks"), patch(__name__ + ".require_full_history"):
                with patch(__name__ + ".working_tree_paths", return_value=["links/escape.txt"]):
                    with redirect_stdout(output):
                        status = run_check(root)
            self.assertEqual(status, 1)
            self.assertEqual(output.getvalue(),
                             "G-10: links/escape.txt is a symlink that points outside the repository\n")

    def test_worktree_selection_includes_tracked_files_even_when_ignored(self):
        with patch("subprocess.run") as run:
            run.return_value.stdout = "tracked.txt\0ignored-but-tracked.txt\0new.txt\0"
            run.return_value.returncode = 0
            self.assertEqual(working_tree_paths(Path(".")), [
                "ignored-but-tracked.txt", "new.txt", "tracked.txt",
            ])
            command = run.call_args[0][0]
            for argument in ("--cached", "--others", "--exclude-standard", "-z"):
                self.assertIn(argument, command)

    def test_real_git_ignores_fake_key_but_scans_the_same_key_in_a_new_file(self):
        require_gitleaks()
        repository = Path(__file__).resolve().parent.parent
        # A disposable subtree uses the real index read-only; it needs no temporary commits.
        with tempfile.TemporaryDirectory(dir=repository / "tools", prefix="secrets-fixture-") as directory:
            root = Path(directory)
            (root / ".gitignore").write_text("ignored.txt\n", encoding="utf-8")
            fake = "sk-" + "0123456789abcdef" * 2
            (root / "ignored.txt").write_text(fake, encoding="utf-8")
            config = repository / ".gitleaks.toml"
            clean = scan_working_tree(root, config)
            self.assertEqual(clean.returncode, 0, clean.stdout)
            (root / "visible.txt").write_text(fake, encoding="utf-8")
            found = scan_working_tree(root, config)
            self.assertEqual(found.returncode, 1, found.stdout)
            self.assertIn("visible.txt", found.stdout)
            self.assertNotIn("ignored.txt", found.stdout)
            self.assertNotIn(fake, found.stdout)


def main():
    """Run both scans, or exercise the scanner against synthetic fixtures."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(SecretsTests)
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1
    return run_check(Path(__file__).resolve().parent.parent)


if __name__ == "__main__":
    raise SystemExit(main())
