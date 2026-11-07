"""Compare two web images file by file; they are reproducible when only Next.js's per-build secrets differ.

Next.js writes fresh random keys into these manifests on every build (preview mode and server-action encryption); the
project never pins them, so a reproducible build is identical everywhere else. Docker also writes a few files into every
created container. Usage: python3 deploy/tests/compare_images.py IMAGE_A IMAGE_B
"""

import hashlib
import subprocess
import sys
import tarfile

PER_BUILD_SECRETS = frozenset({
    "app/.next/prerender-manifest.json",
    "app/.next/server/server-reference-manifest.js",
    "app/.next/server/server-reference-manifest.json",
})
CONTAINER_RUNTIME = frozenset({".dockerenv", "etc/hostname", "etc/hosts", "etc/resolv.conf"})
CONTAINER_RUNTIME_PREFIXES = ("dev/", "proc/", "sys/")
MAX_REPORTED = 20
EXPORT_TIMEOUT_SECONDS = 600


def ignored(path):
    return (path in PER_BUILD_SECRETS or path in CONTAINER_RUNTIME
            or path.startswith(CONTAINER_RUNTIME_PREFIXES))


def index(tar):
    """Maps each path of a filesystem export to its content hash, mode, owner, link target and mtime."""
    entries = {}
    for member in tar:
        path = member.name[2:] if member.name.startswith("./") else member.name
        if ignored(path):
            continue
        digest = ""
        if member.isfile():
            digest = hashlib.sha256(tar.extractfile(member).read()).hexdigest()
        entries[path] = {"content": (member.type, digest, member.linkname), "mode": member.mode,
                         "owner": (member.uid, member.gid), "mtime": member.mtime}
    return entries


def differences(first, second):
    """Human-readable differences between two indexed exports, in path order."""
    found = []
    for path in sorted(set(first) | set(second)):
        if path not in second:
            found.append("{}: only in first".format(path))
        elif path not in first:
            found.append("{}: only in second".format(path))
        else:
            found += ["{}: {} differs".format(path, field) for field in ("content", "mode", "owner", "mtime")
                      if first[path][field] != second[path][field]]
    return found


def export(image):
    container = subprocess.run(["docker", "create", image], check=True, capture_output=True, text=True,
                               timeout=EXPORT_TIMEOUT_SECONDS).stdout.strip()
    try:
        with subprocess.Popen(["docker", "export", container], stdout=subprocess.PIPE) as process:
            with tarfile.open(fileobj=process.stdout, mode="r|") as tar:
                entries = index(tar)
            if process.wait(timeout=EXPORT_TIMEOUT_SECONDS):
                raise RuntimeError("docker export failed for " + image)
        return entries
    finally:
        subprocess.run(["docker", "rm", container], check=True, capture_output=True, timeout=EXPORT_TIMEOUT_SECONDS)


def main(arguments):
    if len(arguments) != 2:
        raise SystemExit("usage: compare_images.py IMAGE_A IMAGE_B")
    found = differences(export(arguments[0]), export(arguments[1]))
    for line in found[:MAX_REPORTED]:
        print(line)
    if found:
        raise SystemExit("{} differences outside the per-build secrets".format(len(found)))
    print("Reproducible: identical files except the Next.js per-build secret manifests")


if __name__ == "__main__":
    main(sys.argv[1:])
