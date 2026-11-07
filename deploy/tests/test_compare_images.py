"""Reproducible web image: the file-level comparison ignores exactly Next.js's per-build secret manifests."""

import io
import tarfile
import unittest

import compare_images


def archive(files):
    """A small in-memory filesystem export: {path: (content, mtime)}."""
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w") as tar:
        for path, (content, mtime) in files.items():
            info = tarfile.TarInfo(path)
            info.size, info.mtime, info.mode = len(content), mtime, 0o644
            tar.addfile(info, io.BytesIO(content))
    buffer.seek(0)
    return compare_images.index(tarfile.open(fileobj=buffer, mode="r|"))


BASE = {"app/server.js": (b"server", 0), "app/.next/BUILD_ID": (b"abc", 0),
        "app/.next/prerender-manifest.json": (b"keys-1", 0),
        "app/.next/server/server-reference-manifest.js": (b"key-1", 0),
        "app/.next/server/server-reference-manifest.json": (b"key-1", 0),
        ".dockerenv": (b"", 1), "etc/hostname": (b"one", 1)}


class CompareImagesTests(unittest.TestCase):
    def test_identical_except_the_secret_manifests_and_container_runtime_files(self):
        other = dict(BASE)
        other.update({"app/.next/prerender-manifest.json": (b"keys-2", 5),
                      "app/.next/server/server-reference-manifest.js": (b"key-2", 5),
                      "app/.next/server/server-reference-manifest.json": (b"key-2", 5),
                      ".dockerenv": (b"", 2), "etc/hostname": (b"two", 2)})
        self.assertEqual(compare_images.differences(archive(BASE), archive(other)), [])

    def test_content_mtime_and_presence_differences_are_reported(self):
        for change, expected in (({"app/server.js": (b"changed", 0)}, "content"),
                                 ({"app/server.js": (b"server", 9)}, "mtime"),
                                 ({"app/extra.js": (b"x", 0)}, "only in second")):
            other = dict(BASE)
            other.update(change)
            found = compare_images.differences(archive(BASE), archive(other))
            self.assertEqual(len(found), 1, change)
            self.assertIn(expected, found[0])

    def test_only_the_exact_manifest_paths_are_ignored(self):
        other = dict(BASE)
        other["app/public/prerender-manifest.json"] = (b"x", 0)
        self.assertEqual(len(compare_images.differences(archive(BASE), archive(other))), 1)


if __name__ == "__main__":
    unittest.main()
