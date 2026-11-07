"""The release artefacts and what each one is made of, read from the Dockerfiles' own pins so the SBOM cannot drift from
the build: the crates each image ships, its npm trees, the components added outside a package manager, and the
project's own files.
"""

from dataclasses import dataclass, field, replace
from pathlib import Path
import re
import tomllib

import sbom_check as check
import sbom_sources as sources

REPOSITORY = Path(__file__).resolve().parents[2]
MUSL = "x86_64-unknown-linux-musl"
GLIBC = "x86_64-unknown-linux-gnu"


@dataclass(frozen=True)
class Crate:
    name: str
    target: str


@dataclass(frozen=True)
class Artefact:
    name: str
    version: str  # the image variant: "base", "workstation", or the image's own name
    crates: list
    own_paths: list  # the project's own files in the image (binaries, scripts, the web app)
    npm_dirs: list = field(default_factory=list)
    gated_npm_dirs: list = field(default_factory=list)
    components: list = field(default_factory=list)  # names of the declared components below
    musl_from: str = ""  # the Alpine build image whose musl is linked statically
    kind: str = "image"  # "image", or "rootfs" for a template's microVM root filesystem


def _read(relative):
    return (REPOSITORY / relative).read_text(encoding="utf-8")


def _pin(text, pattern, what):
    match = re.search(pattern, text, re.MULTILINE)
    if not match:
        raise ValueError("no pin for " + what)
    return match.group(1)


def rust_toolchain():
    return tomllib.loads(_read("rust-toolchain.toml"))["toolchain"]["channel"]


def rust_std():
    version = rust_toolchain()
    return sources.Package(
        name="rust-std", version=version, kind="generic", license="Apache-2.0 OR MIT",
        download_location="https://static.rust-lang.org/dist/rustc-{}-src.tar.xz".format(version))


def node(dockerfile, notice):
    """Node from the pinned official image; its LICENSE is MIT plus the notices of everything it bundles."""
    version = _pin(_read(dockerfile), r"node:(\d+\.\d+\.\d+)-", "Node in " + dockerfile)
    return sources.Package(
        name="node", version=version, kind="generic", license="LicenseRef-node", license_notice=notice,
        download_location="https://nodejs.org/dist/v{0}/node-v{0}.tar.xz".format(version))


def go():
    text = _read("images/runtime/Dockerfile")
    version = _pin(text, r"^ARG GO_VERSION=(\S+)", "Go")
    # The Dockerfile's checksum pins the binary archive, not this source archive, so it is not recorded here.
    return sources.Package(
        name="go", version=version, kind="generic", license="BSD-3-Clause",
        download_location="https://go.dev/dl/go{}.src.tar.gz".format(version))


def firecracker():
    text = _read("images/vmm/Dockerfile")
    version = _pin(text, r"^ARG FIRECRACKER_VERSION=(\S+)", "Firecracker")
    return sources.Package(
        name="firecracker", version=version.lstrip("v"), kind="generic", license="Apache-2.0",
        download_location="https://github.com/firecracker-microvm/firecracker/archive/refs/tags/{}.tar.gz".format(
            version))


def project(version, paths):
    """The project's own files in an image (scripts, the web app): one component under the project's licence."""
    return check.Declared(package=sources.Package(
        name="tensorquay-agent", version=version, kind="generic", license="Apache-2.0",
        download_location=sources.PROJECT_SOURCE), paths=paths)


RUNTIME = "images/runtime/Dockerfile"

ARTEFACTS = {
    "gateway": Artefact(name="tqa-gateway", version="gateway", crates=[Crate("tqa-gateway", MUSL)],
                        own_paths=["/tqa-gateway"], musl_from="images/gateway/Dockerfile"),
    "web": Artefact(name="tqa-web", version="web", crates=[], own_paths=["/app/"],
                    npm_dirs=["app/node_modules"], gated_npm_dirs=["app/node_modules"], components=["node-web"]),
    "runtime-base": Artefact(name="tqa-runtime", version="base", crates=[Crate("tqa-runtime", GLIBC)],
                             own_paths=["/usr/local/bin/tqa-runtime"]),
    "runtime-workstation": Artefact(
        name="tqa-runtime", version="workstation", crates=[Crate("tqa-runtime", GLIBC)],
        own_paths=["/usr/local/bin/tqa-runtime", "/usr/local/bin/go", "/usr/local/lib/tqa/packages.py",
                   "/usr/local/lib/tqa/sbom_sources.py"],
        npm_dirs=["usr/local/lib/node_modules", "usr/local/lib/tqa/browser/node_modules"],
        gated_npm_dirs=["usr/local/lib/tqa/browser/node_modules"], components=["node-runtime", "go"]),
    # The Desktop (ADR-0039): the Workstation with a display stack from Debian and its own init script.
    "runtime-desktop": Artefact(
        name="tqa-runtime", version="desktop", crates=[Crate("tqa-runtime", GLIBC)],
        own_paths=["/usr/local/bin/tqa-runtime", "/usr/local/bin/go", "/usr/local/lib/tqa/packages.py",
                   "/usr/local/lib/tqa/sbom_sources.py", "/usr/local/bin/tqa-desktop-init"],
        npm_dirs=["usr/local/lib/node_modules", "usr/local/lib/tqa/browser/node_modules"],
        gated_npm_dirs=["usr/local/lib/tqa/browser/node_modules"], components=["node-runtime", "go"]),
    "vmm": Artefact(name="tqa-vmm", version="vmm", crates=[Crate("tqa-vm-init", MUSL)],
                    own_paths=["/usr/local/lib/tqa/"], components=["firecracker"], musl_from="images/vmm/Dockerfile"),
}


def _rootfs(source):
    """A built-in template's rootfs: its source image's tree plus the static guest init (images/rootfs/mkrootfs.sh)."""
    image = ARTEFACTS[source]
    return replace(image, name="tqa-rootfs", kind="rootfs", crates=image.crates + [Crate("tqa-vm-init", MUSL)],
                   own_paths=image.own_paths + ["/usr/sbin/tqa-vm-init"], musl_from="images/vmm/Dockerfile")


ARTEFACTS["rootfs-base"] = _rootfs("runtime-base")
ARTEFACTS["rootfs-workstation"] = _rootfs("runtime-workstation")
ARTEFACTS["rootfs-desktop"] = _rootfs("runtime-desktop")


def declared(artefact, root, release):
    """The artefact's declared components with the paths each owns."""
    found = [project(release, artefact.own_paths)]
    if artefact.crates:
        found.append(check.Declared(package=rust_std(), paths=[]))
    for name in artefact.components:
        if name == "node-web":
            found.append(check.Declared(package=node("images/web/Dockerfile", "/usr/share/doc/node/LICENSE"),
                                        paths=["/usr/local/bin/node"]))
        elif name == "node-runtime":
            found.append(check.Declared(package=node(RUNTIME, "/usr/share/doc/node/LICENSE"),
                                        paths=["/usr/local/bin/node"]))
        elif name == "go":
            found.append(check.Declared(package=go(), paths=["/usr/local/go/"]))
        elif name == "firecracker":
            found.append(check.Declared(package=firecracker(), paths=["/usr/local/bin/firecracker"]))
        else:
            raise ValueError("unknown component " + name)
    return found


def musl_image(artefact):
    """The pinned Alpine build image an artefact's static binary links musl from."""
    return _pin(_read(artefact.musl_from), r"^FROM (rust:\S+-alpine@sha256:[0-9a-f]{64})", "the Alpine build image")


def musl(installed):
    """musl from an Alpine package database (`lib/apk/db/installed`): statically linked, so shipped."""
    for block in installed.split("\n\n"):
        fields = dict(line.split(":", 1) for line in block.splitlines() if len(line) > 2 and line[1] == ":")
        if fields.get("P") == "musl":
            version = fields["V"]
            return sources.Package(
                name="musl", version=version, kind="generic", license="MIT",
                download_location="https://musl.libc.org/releases/musl-{}.tar.gz".format(version.split("-r")[0]))
    raise ValueError("no musl in the Alpine build image")
