#!/usr/bin/env python3
"""Release SBOMs (ADR-0017 point 7): for each built image, template rootfs and the guest kernel, an SPDX 2.3 document
and its readable package list, written only when the completeness check passes.

    python3 tools/release/sbom.py --artefact runtime-base --image tqa-runtime:base --release 0.1.0 --out dist/0.1.0/sbom
    python3 tools/release/sbom.py --artefact rootfs-base --rootfs <storage.root>/images/<id> --release 0.1.0 --out ...
    python3 tools/release/sbom.py --artefact kernel --kernel <kernel build dir> --release 0.1.0 --out ...

Offline: the image is read from the local engine, the Rust closure from `cargo --offline --locked`, and every source is
a link, never fetched.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile

import sbom_artefacts as artefacts
import sbom_check as check
import sbom_firecracker as firecracker
import sbom_sources as sources
import sbom_spdx as spdx

TIMEOUT = 600


def run(*arguments, cwd=None):
    return subprocess.run(arguments, check=True, capture_output=True, text=True, timeout=TIMEOUT, cwd=cwd).stdout


def export(image, destination):
    """The image's filesystem, extracted from `docker export` without running the image."""
    container = run("docker", "create", "--network", "none", image, "true").strip()
    try:
        with subprocess.Popen(["docker", "export", container], stdout=subprocess.PIPE) as process:
            with tarfile.open(fileobj=process.stdout, mode="r|") as archive:
                for member in archive:
                    if member.isdev() or member.isfifo():
                        continue
                    archive.extract(member, destination, filter="tar")
            if process.wait(timeout=TIMEOUT) != 0:
                raise RuntimeError("docker export failed for " + image)
    finally:
        run("docker", "rm", "-f", container)


def crates(artefact):
    """The crates each shipped binary links: `cargo tree` without dev, build or proc-macro edges."""
    metadata = json.loads(run("cargo", "metadata", "--offline", "--locked", "--format-version", "1",
                              cwd=artefacts.REPOSITORY))
    lock = (artefacts.REPOSITORY / "Cargo.lock").read_text(encoding="utf-8")
    found = {}
    for crate in artefact.crates:
        tree = run("cargo", "tree", "--offline", "--locked", "-p", crate.name, "--target", crate.target,
                   "-e", "normal,no-proc-macro", "--prefix", "none", cwd=artefacts.REPOSITORY)
        for package in sources.cargo_packages(metadata, tree, lock):
            found[(package.name, package.version)] = package
    return [found[key] for key in sorted(found)]


def policy():
    import tomllib
    deny = tomllib.loads((artefacts.REPOSITORY / "deny.toml").read_text(encoding="utf-8"))["licenses"]
    web = json.loads((artefacts.REPOSITORY / "web/licenses.json").read_text(encoding="utf-8"))
    exceptions = {item["name"]: item.get("allow", []) for item in deny.get("exceptions", [])}
    return check.Policy(allowed=frozenset(deny["allow"]), exceptions=exceptions,
                        forbidden_npm=web.get("forbidden", []), allowed_npm=frozenset(web.get("allow", [])))


def describe(artefact, root, digest, release, created):
    """The document for an artefact's extracted tree, and the check's findings."""
    packages = sources.debian_packages(root) if (root / "var/lib/dpkg/status").is_file() else []
    packages += sources.npm_packages(root, artefact.npm_dirs)
    shipped = crates(artefact)
    packages += shipped
    declared = artefacts.declared(artefact, root, release)
    if artefact.musl_from:
        installed = run("docker", "run", "--rm", "--network", "none", "--entrypoint", "cat",
                        artefacts.musl_image(artefact), "/lib/apk/db/installed")
        declared.append(check.Declared(package=artefacts.musl(installed), paths=[]))
    packages += [component.package for component in declared]
    subject = spdx.Subject(artefact.name, artefact.version, digest, kind=artefact.kind)
    document = spdx.document(subject, packages, created)
    expectation = check.Expectation(
        root=root, digest=digest, npm_dirs=artefact.npm_dirs, crates=shipped, declared=declared,
        own_binaries=[path for path in artefact.own_paths if not path.endswith("/")], policy=policy(),
        gated_npm_dirs=artefact.gated_npm_dirs)
    return document, check.problems(document, expectation)


def generate(name, image, release, created):
    """An image's document, the check's findings and the image's digest."""
    artefact = artefacts.ARTEFACTS[name]
    if artefact.kind != "image":
        raise ValueError(name + " is not an image")
    digest = run("docker", "image", "inspect", "--format", "{{.Id}}", image).strip()
    with tempfile.TemporaryDirectory(prefix="tqa-sbom-") as directory:
        root = Path(directory)
        export(image, root)
        document, problems = describe(artefact, root, digest, release, created)
    return document, problems, digest


def generate_rootfs(name, directory, release, created):
    """A template rootfs, read back from its ext4 image: named by the manifest's rootfs digest."""
    artefact = artefacts.ARTEFACTS[name]
    if artefact.kind != "rootfs":
        raise ValueError(name + " is not a rootfs")
    manifest = firecracker.rootfs_manifest(directory)
    problems = firecracker.rootfs_problems(directory, manifest)
    digest = "sha256:" + manifest["rootfs"]["sha256"]
    with tempfile.TemporaryDirectory(prefix="tqa-sbom-") as extracted:
        root = Path(extracted)
        firecracker.dump_rootfs(Path(directory) / manifest["rootfs"]["file"], root)
        document, found = describe(artefact, root, digest, release, created)
    return document, problems + found, digest


def generate_kernel(directory, created):
    """The guest kernel: Linux at the pinned release, named by its vmlinux digest."""
    pins = firecracker.kernel_pins()
    problems = firecracker.kernel_problems(directory, pins)
    manifest = json.loads((Path(directory) / "manifest.json").read_text(encoding="utf-8"))
    digest = "sha256:" + manifest["vmlinux_sha256"]
    subject = spdx.Subject("tqa-kernel", pins.version, digest, kind="kernel")
    document = spdx.document(subject, [firecracker.kernel_package(pins)], created)
    return document, problems + check.document_problems(document), digest


def write(out, name, document, digest, kernel_dir=None):
    """The artefact's SPDX document and package list, and its entry in the release's `index.json`. The kernel's
    configuration is published beside its SBOM, with the exact source it was built from (GPL-2.0, design D5)."""
    out.mkdir(parents=True, exist_ok=True)
    files = {"spdx": "{}.spdx.json".format(name), "packages": "{}.packages.md".format(name)}
    if kernel_dir is not None:
        files["config"] = "{}.config".format(name)
        (out / files["config"]).write_bytes((Path(kernel_dir) / "config").read_bytes())
        linux = document["packages"][1]
        files["source"] = linux["downloadLocation"]
        files["source_sha256"] = linux["checksums"][0]["checksumValue"]
    (out / files["spdx"]).write_text(spdx.dumps(document), encoding="utf-8")
    (out / files["packages"]).write_text(spdx.package_list(document), encoding="utf-8")
    index_path = out / "index.json"
    index = json.loads(index_path.read_text(encoding="utf-8")) if index_path.is_file() else {}
    subject = document["packages"][0]
    index[name] = dict(files, image=subject["name"], variant=subject["versionInfo"], digest=digest,
                       package_count=len(document["packages"]) - 1)
    index_path.write_text(json.dumps(index, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--artefact", required=True, choices=sorted(artefacts.ARTEFACTS) + ["kernel"])
    given = parser.add_mutually_exclusive_group(required=True)
    given.add_argument("--image", help="the built image, by tag or ID")
    given.add_argument("--rootfs", type=Path, help="a built rootfs directory (rootfs.ext4 and manifest.json)")
    given.add_argument("--kernel", type=Path, help="the kernel build directory (vmlinux, config, manifest.json)")
    parser.add_argument("--release", required=True, help="the release version the project's own files carry")
    parser.add_argument("--out", required=True, type=Path, help="the release's sbom directory (created)")
    args = parser.parse_args()
    created = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))
    kind = "kernel" if args.artefact == "kernel" else artefacts.ARTEFACTS[args.artefact].kind
    source = {"image": args.image, "rootfs": args.rootfs, "kernel": args.kernel}[kind]
    if source is None:
        parser.error("{} needs --{}".format(args.artefact, kind))
    if kind == "kernel":
        document, problems, digest = generate_kernel(source, created)
    elif kind == "rootfs":
        document, problems, digest = generate_rootfs(args.artefact, source, args.release, created)
    else:
        document, problems, digest = generate(args.artefact, source, args.release, created)
    if problems:
        print("{}: the SBOM is incomplete, nothing written".format(args.artefact), file=sys.stderr)
        for problem in problems:
            print("  " + problem, file=sys.stderr)
        return 1
    write(args.out, args.artefact, document, digest, kernel_dir=args.kernel)
    print("{}: {} packages, {}".format(args.artefact, len(document["packages"]) - 1, digest))
    return 0


if __name__ == "__main__":
    sys.exit(main())
