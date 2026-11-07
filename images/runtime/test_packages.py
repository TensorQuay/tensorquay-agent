"""The Workstation inventory renders the release SBOM library's packages; run with unittest discovery in this
directory."""
from pathlib import Path
import json
import tempfile
import unittest

import packages
from test_sbom import image_tree, write


class Inventory(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        image_tree(self.root)

    def test_debian_rows_carry_the_exact_licence_or_a_reference_never_a_guess(self):
        rows = packages.debian_rows(self.root)
        self.assertEqual(len(rows), 4)  # installed packages only
        bash = next(row for row in rows if row.startswith("| bash |"))
        self.assertIn("| GPL-3.0-or-later |", bash)
        self.assertIn("https://snapshot.debian.org/package/bash/5.2.37-2%2Bb5/", bash)
        self.assertIn("https://sources.debian.org/src/bash/5.2.37-2%2Bb5/debian/copyright/", bash)
        libc = next(row for row in rows if row.startswith("| libc6 |"))
        self.assertIn("| LicenseRef-debian-libc6 |", libc)
        self.assertIn("/src/glibc/2.41-12/", libc)

    def test_the_snapshot_is_read_from_the_image_apt_sources_not_repeated(self):
        sources = self.root / "etc/apt/sources.list"
        sources.parent.mkdir(parents=True, exist_ok=True)
        sources.write_text("deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/20260927T000000Z/ "
                           "trixie main\ndeb [check-valid-until=no] https://snapshot.debian.org/archive/"
                           "debian-security/20260927T000000Z/ trixie-security main\n")
        self.assertEqual(packages.snapshot(self.root), "20260927T000000Z")
        sources.write_text("deb http://deb.debian.org/debian trixie main\n")
        with self.assertRaisesRegex(SystemExit, "snapshot"):
            packages.snapshot(self.root)

    def test_an_unknown_notice_fails_instead_of_inventing_a_licence(self):
        (self.root / "usr/share/doc/bash/copyright").unlink()
        with self.assertRaises(ValueError):
            packages.debian_rows(self.root)

    def test_npm_rows_list_the_package_roots_of_both_trees(self):
        write(self.root, "usr/local/lib/node_modules/npm/package.json",
              json.dumps({"name": "npm", "version": "11.6.0", "license": "Artistic-2.0"}))
        write(self.root, "usr/local/lib/tqa/browser/node_modules/playwright/package.json",
              json.dumps({"name": "playwright", "version": "1.63.0", "license": "Apache-2.0"}))
        self.assertEqual(packages.node_rows(self.root), [
            "| npm | 11.6.0 | Artistic-2.0 | [source](https://registry.npmjs.org/npm/-/npm-11.6.0.tgz) |",
            "| playwright | 1.63.0 | Apache-2.0 | "
            "[source](https://registry.npmjs.org/playwright/-/playwright-1.63.0.tgz) |",
        ])

    def test_cells_cannot_break_the_table(self):
        self.assertEqual(packages.cell("a|b\nc"), "a&#124;b c")


if __name__ == "__main__":
    unittest.main()
