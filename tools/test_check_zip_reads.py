"""SEC-063 (G-05-zip): no source under `crates/` calls zip's extraction API, the only code that RUSTSEC-2026-0367,
-0368 and -0369 reach, and `deny.toml`'s ignore of those three goes as soon as the lock can drop zip 8 (calamine)."""

import importlib.util
from pathlib import Path
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parent / "check-zip-reads.py"
SPEC = importlib.util.spec_from_file_location("check_zip_reads", SOURCE)
check_zip_reads = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_zip_reads)

READS = """use std::io::Cursor;
fn open(bytes: &[u8]) {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let entry = archive.by_name("content.xml").unwrap();
    // archive.extract(dir) is what the advisories are about; a comment may name it.
}
"""
DENY = """[advisories]
version = 2
ignore = [
    { id = "RUSTSEC-2026-0367", reason = "extract only" },
    { id = "RUSTSEC-2026-0368", reason = "extract only" },
    { id = "RUSTSEC-2026-0369", reason = "extract only" },
]
"""
NO_IGNORE = """[advisories]
version = 2
ignore = []
"""


def lock(zip_versions=("8.6.0",), calamine="0.36.1"):
    """A Cargo.lock with the given zip versions and calamine version."""
    packages = [f'[[package]]\nname = "zip"\nversion = "{version}"\n' for version in zip_versions]
    packages.append(f'[[package]]\nname = "calamine"\nversion = "{calamine}"\n')
    return "version = 4\n\n" + "\n".join(packages)


def run(sources=None, deny=DENY, lockfile=None):
    """Checks a temporary tree; returns the list of problems."""
    with tempfile.TemporaryDirectory() as root:
        for name, text in (sources or {"crates/tqa-gateway/src/files/sheet.rs": READS}).items():
            Path(root, name).parent.mkdir(parents=True, exist_ok=True)
            Path(root, name).write_text(text)
        Path(root, "deny.toml").write_text(deny)
        Path(root, "Cargo.lock").write_text(lockfile if lockfile is not None else lock())
        return check_zip_reads.check_tree(Path(root))


class ExtractionApi(unittest.TestCase):
    def test_reads_only_pass(self):
        self.assertEqual(run(), [])

    def test_archive_extract_is_refused(self):
        problems = run({"crates/a/src/x.rs": READS + "fn f(a: &mut zip::ZipArchive<R>) { a.extract(\"out\").ok(); }\n"})
        self.assertEqual(len(problems), 1)
        self.assertIn("crates/a/src/x.rs:7", problems[0])
        self.assertIn(".extract(", problems[0])

    def test_every_extraction_name_is_refused(self):
        for code in ("use zip::read::ZipStreamReader;", "use zip::read::stream::ZipStreamReader;",
                     "zip::read::read_zipfile_from_stream(&mut r);", "a.extract_unwrapped_root_dir(d, f);",
                     "a.extract_with_options(d, o);", "zip::ZipArchive::extract(&mut a, d);"):
            with self.subTest(code=code):
                problems = run({"crates/a/tests/t.rs": f"fn f() {{ {code} }}\n"})
                self.assertEqual(len(problems), 1, problems)

    def test_extract_in_a_file_without_zip_is_not_zip(self):
        self.assertEqual(run({"crates/a/src/h.rs": "async fn h(p: Parts) { p.extract::<Query<Q>>().await; }\n"}), [])

    def test_a_split_call_is_still_found(self):
        problems = run({"crates/a/src/x.rs": "fn f(a: &mut zip::ZipArchive<R>) {\n    a\n        .extract(dir)\n}\n"})
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("crates/a/src/x.rs:3", problems[0])

    def test_only_rust_sources_under_crates_are_read(self):
        self.assertEqual(run({"crates/a/src/x.rs": READS, "web/x.rs": "zip::read::ZipStreamReader",
                              "crates/a/README.md": "zip archive.extract(dir)"}), [])


class StaleIgnore(unittest.TestCase):
    def test_ignore_holds_while_calamine_keeps_zip_8(self):
        self.assertEqual(run(lockfile=lock(("8.6.0", "9.0.4"))), [])

    def test_ignore_without_zip_8_in_the_lock_must_go(self):
        problems = run(lockfile=lock(("9.0.4",), calamine="0.37.0"))
        self.assertTrue(any("remove" in problem and "RUSTSEC-2026-0367" in problem for problem in problems), problems)

    def test_a_new_calamine_is_looked_at(self):
        problems = run(lockfile=lock(("8.6.0",), calamine="0.36.2"))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("calamine 0.36.2", problems[0])

    def test_no_ignore_needs_no_note(self):
        self.assertEqual(run(deny=NO_IGNORE, lockfile=lock(("9.0.4",), calamine="0.37.0")), [])

    def test_unreadable_files_are_problems(self):
        self.assertTrue(run(deny="[advisories\n"))
        self.assertTrue(run(lockfile="not = [toml"))


class RealTree(unittest.TestCase):
    def test_the_repository_passes(self):
        self.assertEqual(check_zip_reads.check_tree(Path(__file__).resolve().parent.parent), [])


if __name__ == "__main__":
    unittest.main()
