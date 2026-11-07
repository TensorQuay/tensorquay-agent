"""The release manifest (REL-U-006): release.json, the pins read from the exported tree of the release commit (never
the checkout running the pipeline), and SHA256SUMS."""

import hashlib
from pathlib import Path
import re

SCHEMA = 1
PINS = (
    ("rust", "rust-toolchain.toml", r'^channel\s*=\s*"([^"]+)"', "Rust"),
    ("node", "images/web/Dockerfile", r"node:(\d+\.\d+\.\d+)-", "Node.js"),
    ("go", "images/runtime/Dockerfile", r"^ARG GO_VERSION=(\S+)", "Go"),
    ("firecracker", "images/vmm/Dockerfile", r"^ARG FIRECRACKER_VERSION=v?(\S+)", "Firecracker"),
    ("kernel", "images/rootfs/kernel/Dockerfile", r"^ARG KERNEL_VERSION=(\S+)", "the kernel"),
    ("debian_snapshot", "images/runtime/Dockerfile", r"snapshot\.debian\.org/archive/debian/(\d{8}T\d{6}Z)/",
     "the Debian snapshot"),
)


def _pin(root, relative, pattern, what):
    match = re.search(pattern, (Path(root) / relative).read_text(encoding="utf-8"), re.MULTILINE)
    if not match:
        raise ValueError("no pin for {} in {}".format(what, relative))
    return match.group(1)


def pins(root):
    return {key: _pin(root, relative, pattern, what) for key, relative, pattern, what in PINS}


def protocol_revision(root):
    return int(_pin(root, "crates/tqa-types/src/limits.rs", r"PROTOCOL_REVISION: u16 = (\d+);", "the protocol"))


def sha256sums(directory):
    """`sha256sum` lines for every file under `directory`, sorted by path; SHA256SUMS itself left out."""
    root = Path(directory)
    lines = []
    for path in sorted(item for item in root.rglob("*") if item.is_file()):
        relative = path.relative_to(root).as_posix()
        if relative == "SHA256SUMS":
            continue
        digest = hashlib.sha256()
        with path.open("rb") as stream:
            for block in iter(lambda: stream.read(1 << 20), b""):
                digest.update(block)
        lines.append("{}  {}".format(digest.hexdigest(), relative))
    return "\n".join(lines) + "\n"


def release(*, version, commit, commit_time, tree, releasable, tag, runner_commit, pins, protocol_revision, images,
            rootfs, kernel, vulnerabilities, evaluations):
    counts = {"HIGH": 0, "MEDIUM": 0, "OPEN": 0}
    for image in vulnerabilities.get("counts", {}).values():
        for level, count in image.items():
            counts[level] = counts.get(level, 0) + count
    return {
        "schema": SCHEMA, "version": version, "commit": commit, "commit_time": commit_time, "tree": tree,
        "releasable": releasable, "published": False, "tag": tag, "pipeline_commit": runner_commit,
        "pins": pins, "protocol_revision": protocol_revision, "images": images, "rootfs": rootfs, "kernel": kernel,
        "vulnerabilities": {"passed": vulnerabilities["passed"], "high": counts["HIGH"], "medium": counts["MEDIUM"],
                            "open": counts["OPEN"]},
        "evaluations": [{key: page[key] for key in ("page", "title", "reached")} for page in evaluations],
    }
