#!/usr/bin/env python3
"""Build a microVM rootfs from a local OCI image without host root (ADR-0006, SBX-F-005, SBX-F-010).

Usage: build_rootfs.py --image REF --vmm-image REF --out DIR

The result is DIR/<image id hex>/rootfs.ext4 and manifest.json, both read-only. The build runs inside the VM host
image as the container's namespaced root, so the image's file ownership is preserved. The runtime binary read back
from the built filesystem must equal the one in the source image, or nothing is written.
"""

import argparse
from datetime import datetime
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import uuid

RUNTIME = "/usr/local/bin/tqa-runtime"
REVISION_LABEL = "org.tensorquay.protocol-revision"
TIMEOUT_SECONDS = 1800
HEX64 = re.compile(r"[0-9a-f]{64}")
RESULT_KEYS = {"tree_sha256", "runtime_sha256", "init_sha256", "e2fsprogs"}


class BuildError(Exception):
    """A build step failed; the message never includes image content."""


def docker(*args, timeout=TIMEOUT_SECONDS):
    """Run one bounded docker command and return its standard output."""
    try:
        result = subprocess.run(["docker", *args], capture_output=True, timeout=timeout, check=False)
    except subprocess.TimeoutExpired as error:
        raise BuildError(f"docker {args[0]} timed out") from error
    if result.returncode:
        # The exit code tells a failed step (1-127) from a killed container (128 + the signal, 137 for SIGKILL), which
        # prints nothing on stderr.
        detail = result.stderr.decode(errors="replace").strip().splitlines()[-1:] or [""]
        raise BuildError(f"docker {args[0]} failed (exit {result.returncode}): {detail[0][:200]}")
    return result.stdout


def image_config(inspect):
    """The process configuration an exported filesystem loses; the guest init reads it (tqa-vm-init)."""
    if inspect.get("Os") != "linux" or inspect.get("Architecture") != "amd64":
        raise BuildError("the image must be linux/amd64")
    config = inspect.get("Config") or {}
    user = config.get("User") or ""
    if not re.fullmatch(r"[1-9][0-9]*:[1-9][0-9]*", user):
        raise BuildError("the image user must be a numeric non-root uid:gid")
    command = (config.get("Entrypoint") or []) + (config.get("Cmd") or [])
    if not command or not command[0].startswith("/"):
        raise BuildError("the image needs an absolute entrypoint")
    return {
        "user": user,
        "env": config.get("Env") or [],
        "working_dir": config.get("WorkingDir") or "/",
        "entrypoint": config.get("Entrypoint") or [],
        "cmd": config.get("Cmd") or [],
    }


def protocol_revision(inspect):
    """The image's runtime protocol revision; an unlabelled image is revision 1 (ADR-0026)."""
    label = ((inspect.get("Config") or {}).get("Labels") or {}).get(REVISION_LABEL, "1")
    if not re.fullmatch(r"[1-9][0-9]{0,4}", label):
        raise BuildError("malformed protocol revision label")
    return int(label)


def fs_uuid(image_id):
    """A filesystem UUID and hash seed derived from the image identity, so rebuilds agree."""
    return str(uuid.UUID(hashlib.sha256(image_id.encode()).hexdigest()[:32]))


def source_date_epoch(created):
    """The image's creation time in whole seconds; fractional digits beyond microseconds are dropped."""
    match = re.fullmatch(r"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(\.\d+)?(Z|[+-]\d\d:\d\d)", created or "")
    if not match:
        raise BuildError("malformed image creation time")
    zone = "+00:00" if match.group(3) == "Z" else match.group(3)
    return int(datetime.fromisoformat(match.group(1) + zone).timestamp())


def parse_result(text):
    """Read the in-container step's result, refusing anything unexpected."""
    pairs = dict(line.split("=", 1) for line in text.splitlines() if "=" in line)
    if set(pairs) != RESULT_KEYS or not all(HEX64.fullmatch(pairs[k]) for k in RESULT_KEYS - {"e2fsprogs"}):
        raise BuildError("the rootfs step returned an incomplete result")
    if not re.fullmatch(r"\d+\.\d+(\.\d+)?", pairs["e2fsprogs"]):
        raise BuildError("the rootfs step returned an incomplete result")
    return pairs


def check_lineage(result, image_runtime_sha256):
    """SBX-F-005: the runtime the guest executes is byte for byte the image's."""
    if result["runtime_sha256"] != image_runtime_sha256:
        raise BuildError("the rootfs runtime binary differs from the source image's")


def image_policy(inspect):
    """What the backend's image policy needs (SBX-C-014): the environment's key NAMES, never their values, and the
    declared volumes. The microVM backend refuses what Docker refuses at create: a volume, or a key the spec leaves unset."""
    config = inspect.get("Config") or {}
    keys = sorted({entry.split("=", 1)[0] for entry in config.get("Env") or []})
    return {"env_keys": keys, "volumes": sorted((config.get("Volumes") or {}).keys())}


def manifest(inspect, result, rootfs_sha256, rootfs_bytes, builder_id):
    return {
        # 2: the image policy section. A schema-1 rootfs is refused by the backend and must be rebuilt.
        "schema": 2,
        "source_image": {
            "id": inspect["Id"],
            "repo_digests": sorted(inspect.get("RepoDigests") or []),
            "protocol_revision": protocol_revision(inspect),
        },
        "rootfs": {"file": "rootfs.ext4", "sha256": rootfs_sha256, "bytes": rootfs_bytes,
                   "tree_sha256": result["tree_sha256"]},
        "init": {"path": "/usr/sbin/tqa-vm-init", "sha256": result["init_sha256"]},
        "runtime": {"path": RUNTIME, "sha256": result["runtime_sha256"]},
        "builder": {"image_id": builder_id, "e2fsprogs": result["e2fsprogs"]},
        "image": image_policy(inspect),
    }


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def image_runtime_sha256(image_id):
    """Hash the runtime binary through the engine, independently of the export path."""
    container = docker("create", "--network", "none", image_id).decode().strip()
    try:
        archive = docker("cp", f"{container}:{RUNTIME}", "-")
    finally:
        docker("rm", "-f", container)
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        member = tar.extractfile(Path(RUNTIME).name)
        if member is None:
            raise BuildError("the image has no runtime binary")
        return hashlib.sha256(member.read()).hexdigest()


def export_rootfs(image_id, work):
    container = docker("create", "--network", "none", image_id).decode().strip()
    try:
        docker("export", "-o", str(work / "rootfs.tar"), container)
    finally:
        docker("rm", "-f", container)


def run_builder(vmm_image, work, image_id, epoch):
    # Namespaced root inside a rootless engine is the invoking user on the host; it is needed to keep ownership.
    docker(
        "run", "--rm", "--network", "none", "--user", "0:0", "--cap-drop", "ALL",
        "--cap-add", "CHOWN", "--cap-add", "DAC_OVERRIDE", "--cap-add", "FOWNER", "--cap-add", "FSETID",
        "--security-opt", "no-new-privileges",
        "--mount", f"type=bind,src={work},dst=/work",
        "--mount", "type=volume,dst=/build",
        "-e", f"FS_UUID={fs_uuid(image_id)}", "-e", f"SOURCE_DATE_EPOCH={epoch}",
        "--entrypoint", "/bin/bash", vmm_image, "/usr/local/lib/tqa/mkrootfs.sh",
    )


def e2fsck_refusal(work, out):
    """After a failed build step: when e2fsck refused the new image, keep its whole report beside the output (owner-only)
    and name its exit code. The report stays out of the message, which never includes image content."""
    code_file, log = work / "e2fsck.code", work / "e2fsck.log"
    if not code_file.is_file():
        return None
    code = code_file.read_text().strip()
    if not re.fullmatch(r"\d{1,3}", code):
        code = "unknown"
    kept = out / ".e2fsck-{}-{}.log".format(int(time.time()), os.getpid())
    if log.is_file():
        shutil.copyfile(log, kept)
        kept.chmod(0o600)
        return BuildError(f"e2fsck exited {code} on the new rootfs; its report is in {kept}")
    return BuildError(f"e2fsck exited {code} on the new rootfs; it left no report")


def build(image, vmm_image, out):
    inspect = json.loads(docker("image", "inspect", image))[0]
    image_id = inspect["Id"]
    builder_id = json.loads(docker("image", "inspect", vmm_image))[0]["Id"]
    target = out / image_id.removeprefix("sha256:")
    if target.exists():
        raise BuildError("a rootfs for this image already exists")
    with tempfile.TemporaryDirectory(prefix=".build-", dir=out) as temporary:
        work = Path(temporary)
        (work / "image.json").write_text(json.dumps(image_config(inspect), indent=2) + "\n")
        export_rootfs(image_id, work)
        try:
            run_builder(vmm_image, work, image_id, source_date_epoch(inspect.get("Created")))
        except BuildError as error:
            refusal = e2fsck_refusal(work, out)
            if refusal is None:
                raise
            raise refusal from error
        result = parse_result((work / "result.env").read_text())
        check_lineage(result, image_runtime_sha256(image_id))
        staged = work / "out"
        staged.mkdir()
        rootfs = staged / "rootfs.ext4"
        (work / "rootfs.ext4").rename(rootfs)
        document = manifest(inspect, result, sha256_file(rootfs), rootfs.stat().st_size, builder_id)
        (staged / "manifest.json").write_text(json.dumps(document, indent=2) + "\n")
        for path in (rootfs, staged / "manifest.json"):
            path.chmod(0o444)
        os.rename(staged, target)
    return target


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--image", required=True, help="source image reference or ID")
    parser.add_argument("--vmm-image", required=True, help="the VM host image (images/vmm) used as the builder")
    parser.add_argument("--out", required=True, type=Path, help="existing directory for rootfs artefacts")
    args = parser.parse_args(argv)
    if not args.out.is_dir():
        parser.error("--out must be an existing directory")
    try:
        target = build(args.image, args.vmm_image, args.out.resolve())
    except BuildError as error:
        print(f"build_rootfs: {error}", file=sys.stderr)
        return 1
    print(target)
    return 0


if __name__ == "__main__":
    sys.exit(main())
