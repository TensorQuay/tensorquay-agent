"""REL-U-006: the release manifest's pins (read from the exported tree), release.json and SHA256SUMS (no Docker)."""

import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import release_manifest as manifest

FILES = {
    "rust-toolchain.toml": '[toolchain]\nchannel = "1.97.1"\n',
    "images/web/Dockerfile": "FROM node:24.21.0-trixie-slim@sha256:x AS build\n",
    "images/runtime/Dockerfile": ("ARG GO_VERSION=1.27.1\nRUN printf '%s\\n' \\\n"
                                  "    'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/"
                                  "20260927T000000Z/ trixie main' \\\n"),
    "images/vmm/Dockerfile": "ARG FIRECRACKER_VERSION=v1.17.0\n",
    "images/rootfs/kernel/Dockerfile": "ARG KERNEL_VERSION=6.18.54\n",
    "crates/tqa-types/src/limits.rs": "pub const PROTOCOL_REVISION: u16 = 8;\n",
}


def tree(root, files=FILES):
    for relative, text in files.items():
        path = Path(root) / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")


class Pins(unittest.TestCase):
    def test_every_pin_comes_from_the_exported_tree(self):
        with tempfile.TemporaryDirectory() as root:
            tree(root)
            self.assertEqual(manifest.pins(root), {
                "rust": "1.97.1", "node": "24.21.0", "go": "1.27.1", "firecracker": "1.17.0", "kernel": "6.18.54",
                "debian_snapshot": "20260927T000000Z"})
            self.assertEqual(manifest.protocol_revision(root), 8)

    def test_a_missing_pin_fails_instead_of_guessing(self):
        with tempfile.TemporaryDirectory() as root:
            tree(root, dict(FILES, **{"images/vmm/Dockerfile": "FROM debian\n"}))
            with self.assertRaisesRegex(ValueError, "Firecracker"):
                manifest.pins(root)


class Sums(unittest.TestCase):
    def test_every_file_is_listed_sorted_and_the_list_leaves_itself_out(self):
        with tempfile.TemporaryDirectory() as root:
            (Path(root) / "sbom").mkdir()
            (Path(root) / "sbom" / "gateway.spdx.json").write_bytes(b"{}")
            (Path(root) / "release.json").write_bytes(b"[]")
            (Path(root) / "SHA256SUMS").write_bytes(b"stale")
            text = manifest.sha256sums(root)
            self.assertEqual(text.splitlines(), [
                "{}  release.json".format(hashlib.sha256(b"[]").hexdigest()),
                "{}  sbom/gateway.spdx.json".format(hashlib.sha256(b"{}").hexdigest())])


class Release(unittest.TestCase):
    def test_the_manifest_records_what_the_notes_and_the_product_owner_need(self):
        record = manifest.release(
            version="1.0.0", commit="a" * 40, commit_time="2026-09-28T05:00:00+08:00", tree="t" * 40,
            releasable=True, tag={"tag": "v1.0.0", "state": "to create"}, runner_commit="c" * 40,
            pins={"rust": "1.97.1"}, protocol_revision=8,
            images={"gateway": {"id": "sha256:" + "1" * 64, "size": 1, "labels": {}}},
            rootfs={"base": {"sha256": "2" * 64}}, kernel=None,
            vulnerabilities={"passed": True, "counts": {"web": {"OPEN": 3}}},
            evaluations=[{"page": "gate-7.md", "title": "Gate 7: Channels", "reached": True, "state": "", "limits": []}])
        self.assertEqual(record["schema"], 1)
        self.assertEqual(record["vulnerabilities"], {"passed": True, "high": 0, "medium": 0, "open": 3})
        self.assertEqual(record["evaluations"], [{"page": "gate-7.md", "title": "Gate 7: Channels", "reached": True}])
        self.assertEqual(record["published"], False)
        json.dumps(record)


if __name__ == "__main__":
    unittest.main()
