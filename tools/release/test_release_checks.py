"""REL-U-004: the release pipeline's refusals, on git outputs (no repository, no Docker)."""

import unittest

import release_checks as checks

SHA = "a" * 40
OTHER = "b" * 40


class Version(unittest.TestCase):
    def test_semantic_versions_and_release_candidates_pass(self):
        for version in ("1.0.0", "0.9.0", "10.20.30", "1.0.0-rc.1", "1.0.0-rc.12"):
            with self.subTest(version):
                self.assertEqual(checks.version(version), version)

    def test_anything_else_is_refused(self):
        for version in ("v1.0.0", "1.0", "1.0.0.x", "01.0.0", "1.0.0-beta", "1.0.0-rc.0", "1.0.0-rc", "", "1.0.0 "):
            with self.subTest(version), self.assertRaises(checks.Refused):
                checks.version(version)


class Checkout(unittest.TestCase):
    def test_a_clean_checkout_passes(self):
        checks.clean_checkout("")
        checks.clean_checkout("\n")

    def test_changed_or_untracked_files_are_refused_and_named(self):
        with self.assertRaisesRegex(checks.Refused, "tools/release/release.py"):
            checks.clean_checkout(" M tools/release/release.py\n?? notes.txt\n")


class Tag(unittest.TestCase):
    def test_an_absent_tag_is_to_be_created_by_the_owner(self):
        self.assertEqual(checks.tag("1.0.0", SHA, ""), {"tag": "v1.0.0", "state": "to create"})

    def test_a_tag_on_this_commit_passes(self):
        self.assertEqual(checks.tag("1.0.0", SHA, SHA + "\n"), {"tag": "v1.0.0", "state": "exists"})

    def test_a_moved_tag_is_refused(self):
        with self.assertRaisesRegex(checks.Refused, "v1.0.0 names " + OTHER[:12]):
            checks.tag("1.0.0", SHA, OTHER + "\n")


class Commit(unittest.TestCase):
    def test_a_full_sha_is_required(self):
        self.assertEqual(checks.commit(SHA + "\n"), SHA)
        for bad in ("", "abc", "g" * 40, SHA + "\n" + OTHER):
            with self.subTest(bad), self.assertRaises(checks.Refused):
                checks.commit(bad)

    def test_a_release_comes_from_main_and_a_candidate_says_so(self):
        self.assertTrue(checks.releasable(on_main=True, candidate=False))
        self.assertFalse(checks.releasable(on_main=False, candidate=True))
        self.assertFalse(checks.releasable(on_main=True, candidate=True))
        with self.assertRaisesRegex(checks.Refused, "main"):
            checks.releasable(on_main=False, candidate=False)


class Directory(unittest.TestCase):
    def test_an_existing_release_directory_is_never_overwritten(self):
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as out:
            self.assertEqual(checks.release_directory(Path(out), "1.0.0"), Path(out) / "1.0.0")
            (Path(out) / "1.0.0").mkdir()
            with self.assertRaisesRegex(checks.Refused, "exists"):
                checks.release_directory(Path(out), "1.0.0")
            (Path(out) / "1.0.0").rmdir()
            (Path(out) / ".1.0.0.partial").mkdir()
            with self.assertRaisesRegex(checks.Refused, "partial"):
                checks.release_directory(Path(out), "1.0.0")


if __name__ == "__main__":
    unittest.main()
