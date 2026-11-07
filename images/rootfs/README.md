# MicroVM rootfs and guest kernel

Firecracker needs a block-device root filesystem and an uncompressed kernel (ADR-0006). Both are built locally,
without host root, and are never committed or distributed.

## Rootfs: OCI image to ext4
```sh
docker build -f images/vmm/Dockerfile --target vmm -t tqa-vmm .
python3 images/rootfs/build_rootfs.py --image tqa-runtime:base --vmm-image tqa-vmm --out <storage.root>/images
```
The script:
1. exports a container of the image;
2. extracts it inside the VM host image as the container's namespaced root, which keeps the image's ownership
   (root and uid 1000);
3. adds the static guest init (`/usr/sbin/tqa-vm-init`) and the image's process configuration
   (`/etc/tqa-vm/image.json`: user, environment, working directory, entrypoint), which an exported filesystem does
   not carry;
4. builds the image with `mkfs.ext4 -d`, using a UUID and hash seed derived from the image ID and the image's
   creation time. The result has no journal, because the root is mounted read-only;
5. checks the result with `e2fsck`.

Refusals:
- the image user must be a numeric non-root `uid:gid`, and the entrypoint must be absolute;
- the runtime binary read back from the ext4 image must equal the one in the source image (SBX-F-005), or nothing is
  written;
- an existing output is never replaced.

The output directory is `<out>/<image id>/`, and it holds `rootfs.ext4` and `manifest.json`, both read-only.
Builds are keyed by the image ID. A template pinned by registry digest (`repo@sha256:…`) finds its build through the
manifest's repository digests, so build the rootfs from an image that was pulled or pushed under that digest; exactly one
build may list it (SBX-F-022). The
manifest records:
- the source image ID, repository digests and protocol revision;
- the rootfs SHA-256, its size, and a content tree digest (names, types, modes, owners, sizes, link targets and file
  hashes);
- the init and runtime hashes;
- the builder image ID and e2fsprogs version;
- (schema 2) the image policy inputs: the image environment's key NAMES, never their values, and its declared volumes.
  The microVM backend refuses at create what Docker refuses (SBX-C-014): a declared volume, or an environment key the
  sandbox's environment does not set, so a secret baked in with `ENV` never reaches a guest.

A secret baked in as a *file* is the template check's concern, before a rootfs is built for it: a template whose own
layers hold a credential file of a known shape (a private key, SSH key, or Git, AWS, Docker, npm or PyPI credential) is
refused unless the install file acknowledges its path (ADR-0032, `image_secret_file`). That scan cannot prove absence;
a secret in any other shape reaches the guest like any other file of the image.

**Schema 2 needs a rebuild.** A rootfs built before the image policy (schema 1) is refused with "the rootfs was built by
an older builder; rebuild it (images/rootfs)". Rebuild each template's rootfs with this script after upgrading.

The builder image ID pins the guest init and `mkrootfs.sh`, because both are copied into it.
The VM host image also carries `/usr/local/lib/tqa/firecracker.json` with:
- the Firecracker version;
- the release archive's SHA-256 and that of the published checksum file, both pinned in the Dockerfile and checked at
  build time;
- the installed binary's SHA-256.

## Guest kernel
```sh
docker build -f images/rootfs/kernel/Dockerfile --target kernel --output type=local,dest=<dir> images/rootfs/kernel
```
The build uses:
- Linux 6.18.54 (longterm) from kernel.org;
- Firecracker v1.17.0's `microvm-kernel-ci-x86_64-6.18.config`;
- the committed fragment `kernel/tqa.config`, which turns off loadable modules and devtmpfs automount.

The source archive and the base configuration are verified by SHA-256. `kernel/build.sh` checks that the fragment
and the options the guest init relies on survive `olddefconfig`. The output is `vmlinux`, the final `config` and a
`manifest.json` with every input and output hash. The kernel is GPL-2.0-only. Whether a release ships a kernel
binary is a Product Owner decision; until then it is built locally only.

## Tests
- `python3 -m unittest discover -s images/rootfs -p 'test_*.py'` runs the unit tests (SBX-F-010, SBX-F-005).
- With Docker, set `TQA_DOCKER_TESTS=1` and `TQA_VMM_IMAGE=tqa-vmm`, with `tqa-runtime:base` and
  `tqa-runtime:workstation` built. This builds real rootfs images twice, checks that the manifests are identical,
  and checks ownership, the init and lineage with `debugfs`.
