"""The Firecracker artefacts (ADR-0006): a template's rootfs, read back from its ext4 image, and the guest kernel with
its exact source tarball and the configuration published beside it (design D5).

Both are built locally by an install, never distributed; the release publishes what they are made of.
"""

from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import re
import subprocess

import sbom_sources as sources

REPOSITORY = Path(__file__).resolve().parents[2]
KERNEL_DOCKERFILE = "images/rootfs/kernel/Dockerfile"
KERNEL_FRAGMENT = "images/rootfs/kernel/tqa.config"
TIMEOUT = 600
# debugfs restores owners only as root; the tree is read for its content, so a failed chown is expected.
CHOWN_REFUSED = "while changing ownership of"


@dataclass(frozen=True)
class KernelPins:
    version: str
    source_sha256: str
    firecracker: str
    base_config_sha256: str
    fragment_sha256: str


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def _arg(text, name):
    match = re.search(r"^ARG {}=(\S+)$".format(name), text, re.MULTILINE)
    if not match:
        raise ValueError("no {} pin in {}".format(name, KERNEL_DOCKERFILE))
    return match.group(1)


def kernel_pins():
    """What the kernel Dockerfile pins: the release, its tarball's checksum, the base config and our fragment."""
    text = (REPOSITORY / KERNEL_DOCKERFILE).read_text(encoding="utf-8")
    return KernelPins(version=_arg(text, "KERNEL_VERSION"), source_sha256=_arg(text, "KERNEL_SHA256"),
                      firecracker=_arg(text, "FIRECRACKER_VERSION"), base_config_sha256=_arg(text, "CONFIG_SHA256"),
                      fragment_sha256=sha256_file(REPOSITORY / KERNEL_FRAGMENT))


def kernel_package(pins):
    """Linux itself: GPL-2.0-only, with the exact kernel.org tarball the build verified."""
    major = pins.version.split(".")[0]
    return sources.Package(
        name="linux", version=pins.version, kind="generic", license="GPL-2.0-only", sha256=pins.source_sha256,
        download_location="https://cdn.kernel.org/pub/linux/kernel/v{}.x/linux-{}.tar.xz".format(major, pins.version))


def kernel_problems(directory, pins):
    """The build directory against its manifest (C6: the files are the ones described) and the manifest against the
    Dockerfile's pins (C1: the listed source is the one built)."""
    directory = Path(directory)
    manifest = json.loads((directory / "manifest.json").read_text(encoding="utf-8"))
    problems = []
    for name in ("vmlinux", "config"):
        path = directory / name
        if not path.is_file():
            problems.append("C6: {} is missing".format(name))
        elif sha256_file(path) != manifest.get(name + "_sha256"):
            problems.append("C6: {} is not the file its manifest describes".format(name))
    expected = {"release": pins.version, "source_sha256": pins.source_sha256,
                "base_config_sha256": pins.base_config_sha256, "fragment_sha256": pins.fragment_sha256}
    for key, value in expected.items():
        if manifest.get(key) != value:
            problems.append("C1: the manifest's {} is not the pinned {}".format(key, value))
    return problems


def rootfs_manifest(directory):
    return json.loads((Path(directory) / "manifest.json").read_text(encoding="utf-8"))


def rootfs_problems(directory, manifest):
    """C6: the ext4 image is the one its manifest (and so the SBOM's subject) names."""
    image = Path(directory) / manifest["rootfs"]["file"]
    if not image.is_file() or sha256_file(image) != manifest["rootfs"]["sha256"]:
        return ["C6: {} is not the image its manifest describes".format(manifest["rootfs"]["file"])]
    return []


def dump_rootfs(image, destination):
    """The ext4 image's whole tree, copied out with `debugfs rdump` (read-only, no mount, no root)."""
    result = subprocess.run(["debugfs", "-R", "rdump / {}".format(destination), str(image)],
                            capture_output=True, text=True, timeout=TIMEOUT)
    errors = [line for line in result.stderr.splitlines()
              if line.strip() and not line.startswith("debugfs ") and CHOWN_REFUSED not in line]
    if result.returncode != 0 or errors:
        raise RuntimeError("debugfs could not read {}: {}".format(Path(image).name, "; ".join(errors[:3])))
