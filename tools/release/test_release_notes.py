"""REL-U-005: the release notes skeleton, from the manifest and the gate evaluation pages (no Docker)."""

import pathlib
import unittest

import release_notes as notes

EVALS = pathlib.Path(__file__).resolve().parents[2] / "docs" / "evals"

REACHED = """# Gate 7: Channels

**State:** the gate is **reached**, by the technical lead's evaluation below (2026-09-28, dev `a0bf5ef`).

## Result
Everything passed.

## Limits of the acceptance
- **One:** the first limit,
  which continues here.
- The second limit.

## Evaluation (technical lead, 2026-09-28)
**Verdict: Gate 7 is reached.**
"""

DRAFT = """# Gate 5: Local-model quality (draft)

**State:** draft for the technical lead's acceptance. **The gate failed as measured on 2026-09-26.** More text.

## Limits of the acceptance and next steps
- A draft's limit.
"""

MANIFEST = {
    "version": "0.9.0-rc.1", "commit": "a" * 40, "commit_time": "2026-09-28T05:00:00+08:00", "releasable": False,
    "tag": {"tag": "v0.9.0-rc.1", "state": "to create"}, "protocol_revision": 8,
    "pins": {"rust": "1.97.1", "node": "24.21.0", "go": "1.27.1", "firecracker": "1.17.0", "kernel": "6.18.54",
             "debian_snapshot": "20260927T000000Z"},
    "images": {"gateway": {"id": "sha256:" + "1" * 64, "size": 30_000_000},
               "runtime-workstation": {"id": "sha256:" + "2" * 64, "size": 2_300_000_000}},
    "rootfs": {"base": {"sha256": "3" * 64}}, "kernel": None,
    "vulnerabilities": {"passed": True, "high": 0, "medium": 0, "open": 540},
}


class Evaluations(unittest.TestCase):
    def test_a_reached_page_gives_its_title_state_and_limits(self):
        page = notes.evaluation("gate-7.md", REACHED)
        self.assertEqual(page["title"], "Gate 7: Channels")
        self.assertTrue(page["reached"])
        self.assertEqual(page["limits"], ["**One:** the first limit, which continues here.", "The second limit."])

    def test_a_draft_is_not_reached_and_its_limits_still_count(self):
        page = notes.evaluation("gate-5.md", DRAFT)
        self.assertFalse(page["reached"])
        self.assertEqual(page["state"], "draft for the technical lead's acceptance.")
        self.assertEqual(page["limits"], ["A draft's limit."])

    def test_known_limits_inside_the_evaluation_count_too(self):
        page = notes.evaluation("gate-6.md", """# Gate 6: Extensions

**State:** the gate is **reached**, by the technical lead's evaluation below.

## Evaluation (technical lead, 2026-09-27)
- **Outside this gate:** something else.
- **Known limits, accepted:** SEC-013 does not read process memory;
  documented.
""")
        self.assertEqual(page["limits"], ["**Known limits, accepted:** SEC-013 does not read process memory; documented."])

    def test_a_known_limits_section_counts_too(self):
        page = notes.evaluation("gate-11.md", """# Gate 11: Computer use

**State:** **draft, not reached** (21 Oct 2026, for the technical lead).

## What's still open
- Not a limit.

## Known limits (v1.1)
- **Docker only.** The microVM's rootfs has no Desktop yet,
  so the microVM refuses a desktop sandbox.
- **A second limit.**

## Evaluation (the technical lead)
*Left to the technical lead.*
""")
        self.assertEqual(page["limits"], ["**Docker only.** The microVM's rootfs has no Desktop yet, so the microVM "
                                          "refuses a desktop sandbox.", "**A second limit.**"])

    def test_gate_10s_and_gate_11s_known_limits_are_collected(self):
        for name in ("gate-10.md", "gate-11.md"):
            text = (EVALS / name).read_text(encoding="utf-8")
            lines = text.splitlines()
            start = lines.index(next(line for line in lines if line.startswith("## Known limits")))
            end = next(index for index in range(start + 1, len(lines)) if lines[index].startswith("## "))
            bullets = [line[2:].strip() for line in lines[start + 1:end] if line.startswith("- ")]
            limits = notes.evaluation(name, text)["limits"]
            self.assertTrue(bullets, name)
            for bullet in bullets:
                self.assertTrue(any(limit.startswith(bullet) for limit in limits), (name, bullet))

    def test_a_page_without_a_state_line_is_refused(self):
        with self.assertRaises(ValueError):
            notes.evaluation("gate-9.md", "# Gate 9\n\nNo state here.\n")


class Render(unittest.TestCase):
    def render(self):
        return notes.render(MANIFEST, [notes.evaluation("gate-5.md", DRAFT), notes.evaluation("gate-7.md", REACHED)])

    def test_the_skeleton_names_the_versions_images_and_gates(self):
        text = self.render()
        self.assertTrue(text.startswith("# TensorQuay Agent 0.9.0-rc.1\n"))
        for expected in ("Commit: `" + "a" * 40 + "`", "| Rust | 1.97.1 |", "| Debian snapshot | 20260927T000000Z |",
                         "| gateway | `sha256:" + "1" * 64 + "` |", "| Gate 7: Channels | reached |",
                         "| Gate 5: Local-model quality (draft) | not reached:", "Protocol revision: 8",
                         "0 HIGH, 0 MEDIUM", "not built for this release"):
            self.assertIn(expected, text)

    def test_the_known_limits_come_from_every_page(self):
        text = self.render()
        self.assertIn("- Gate 7: **One:** the first limit, which continues here.", text)
        self.assertIn("- Gate 5: A draft's limit.", text)

    def test_a_candidate_says_it_is_not_releasable_and_publishing_needs_the_owner(self):
        text = self.render()
        self.assertIn("**Candidate dry run: not releasable.**", text)
        self.assertIn("Publishing this release needs the owner's approval.", text)
        self.assertIn("v0.9.0-rc.1 (to create, by the owner)", text)

    def test_the_parts_a_person_writes_are_marked(self):
        text = self.render()
        self.assertIn("## Highlights\n\n_To write._", text)
        self.assertIn("## Breaking changes and upgrade\n\n_To write._", text)


if __name__ == "__main__":
    unittest.main()
