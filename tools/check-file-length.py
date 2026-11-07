#!/usr/bin/env python3
"""G-06: reject source files longer than 1000 lines, excluding generated output."""

import argparse
import os
from pathlib import Path
import sys
import tempfile
import unittest


SOURCE_EXTENSIONS = {".rs", ".ts", ".tsx", ".js", ".mjs", ".cjs", ".py", ".sh", ".yml", ".yaml"}
SKIP_DIRECTORIES = {".git", "target", "node_modules", ".next", "generated"}
LOCK_FILES = {"Cargo.lock", "package-lock.json", "npm-shrinkwrap.json", "pnpm-lock.yaml", "yarn.lock"}
MAX_LINES = 1000


def raise_walk_error(error):
    """Fail closed if a source directory cannot be read."""
    raise error


def check_tree(root):
    """Return sorted relative paths and line counts for files exceeding the limit."""
    offenders = []
    for directory, directories, files in os.walk(root, onerror=raise_walk_error):
        directories[:] = sorted(name for name in directories if name not in SKIP_DIRECTORIES)
        for name in sorted(files):
            path = Path(directory) / name
            if path.suffix not in SOURCE_EXTENSIONS or name in LOCK_FILES or name.endswith(".lock"):
                continue
            with path.open("rb") as source:
                lines = sum(1 for _ in source)
            if lines > MAX_LINES:
                offenders.append((path.relative_to(root).as_posix(), lines))
    return sorted(offenders)


def run_check(root):
    """Print every offending file and return a nonzero status if the gate fails."""
    try:
        offenders = check_tree(root)
    except OSError as error:
        print(f"G-06: cannot read source tree: {error}", file=sys.stderr)
        return 1
    for path, lines in offenders:
        print(f"{path}: {lines} lines (limit {MAX_LINES})")
    if not offenders:
        print(f"G-06: all source files are at most {MAX_LINES} lines")
    return 1 if offenders else 0


class FileLengthTests(unittest.TestCase):
    """Exercise the inclusive limit and excluded paths with temporary source trees."""

    def test_limit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "boundary.rs"
            source.write_text("// line\n" * 1000, encoding="utf-8")
            self.assertEqual(check_tree(root), [])
            source.write_text("// line\n" * 1001, encoding="utf-8")
            self.assertEqual(check_tree(root), [("boundary.rs", 1001)])

    def test_exclusions(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("target", "node_modules", ".next", "nested/generated"):
                source = root / name / "long.rs"
                source.parent.mkdir(parents=True)
                source.write_text("// line\n" * 1001, encoding="utf-8")
            for name in ("Cargo.lock", "package-lock.json", "pnpm-lock.yaml", "notes.md"):
                (root / name).write_text("line\n" * 1001, encoding="utf-8")
            self.assertEqual(check_tree(root), [])

    def test_extensions_and_unterminated_line(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            extensions = ("rs", "ts", "tsx", "js", "mjs", "cjs", "py", "sh", "yml", "yaml")
            for extension in extensions:
                (root / f"long.{extension}").write_text("line\n" * 1000 + "last", encoding="utf-8")
            expected = sorted((f"long.{extension}", 1001) for extension in extensions)
            self.assertEqual(check_tree(root), expected)


def main():
    """Run the gate or its standalone regression tests."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(FileLengthTests)
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1
    return run_check(Path(__file__).resolve().parent.parent)


if __name__ == "__main__":
    raise SystemExit(main())
