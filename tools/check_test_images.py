#!/usr/bin/env python3
"""After a KVM or ext-check test run, no image a test built may be left behind: no `tqa-extcheck-*` image at all, and
at most one `tqa-fc-probe` image per probe variant (the cache keyed by its inputs, SBX-F-034). Reads the engine's
image list; exits 1 naming what is left."""

import re
import subprocess
import sys

# A probe variant's cached image: `tqa-fc-probe:<variant>-<16 hex digits>`.
PROBE = re.compile(r"^tqa-fc-probe:(?P<variant>[a-z-]+)-[0-9a-f]{16}$")


def leftovers(images):
    """The images a test run must not leave: every ext-check one, and every probe variant's beyond its first."""
    found, seen = [], {}
    for image in images:
        repository = image.split(":", 1)[0]
        if repository.startswith("tqa-extcheck-"):
            found.append(image)
            continue
        probe = PROBE.match(image)
        if probe:
            seen.setdefault(probe.group("variant"), []).append(image)
    for variant in sorted(seen):
        if len(seen[variant]) > 1:
            found.extend(sorted(seen[variant]))
    return found


def main():
    listed = subprocess.run(["docker", "image", "ls", "--format", "{{.Repository}}:{{.Tag}}"], check=True,
                            capture_output=True, text=True).stdout.split()
    left = leftovers(listed)
    if left:
        print("test images left behind (each test removes what it builds; one probe image per variant):", file=sys.stderr)
        for image in left:
            print("  " + image, file=sys.stderr)
        return 1
    print("no test images left behind")
    return 0


if __name__ == "__main__":
    sys.exit(main())
