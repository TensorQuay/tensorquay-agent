#!/usr/bin/env bash
# Rootfs build step, run inside the VM host image as the container's namespaced root (never host root), so the
# image's file ownership survives extraction. Input in /work: rootfs.tar (an exported container) and image.json.
# Output in /work: rootfs.ext4 and result.env. Environment: FS_UUID and SOURCE_DATE_EPOCH (ADR-0006, SBX-F-010).
set -euo pipefail

work=/work
root=/build/root
image="$work/rootfs.ext4"
: "${FS_UUID:?}" "${SOURCE_DATE_EPOCH:?}"
trap 'rm -f /tmp/dump' EXIT

mkdir -p "$root"
tar -xpf "$work/rootfs.tar" -C "$root" --numeric-owner --same-owner
rm -f "$root/.dockerenv"
install -D -m 0755 -o 0 -g 0 /usr/local/lib/tqa/tqa-vm-init "$root/usr/sbin/tqa-vm-init"
install -D -m 0444 -o 0 -g 0 "$work/image.json" "$root/etc/tqa-vm/image.json"

# Content identity: names, types, modes, owners, sizes, link targets and file contents; no timestamps or order.
tree=$( (
    cd "$root"
    find . -printf '%P\t%M\t%U\t%G\t%s\t%l\n' | LC_ALL=C sort
    find . -type f -print0 | LC_ALL=C sort -z | xargs -0 -r sha256sum
) | sha256sum | cut -d' ' -f1)

# Sized from the content only (whole 4 KiB blocks per file, one per other entry), never from the build volume's usage.
read -r blocks inodes < <(find "$root" -printf '%y %s\n' \
    | awk '{ n++; b += ($1 == "f") ? int(($2 + 4095) / 4096) : 1 } END { print b, n }')
kib=$((blocks * 4))
# A read-only root needs no journal and no reserved blocks; the headroom covers ext4 metadata.
E2FSPROGS_FAKE_TIME="$SOURCE_DATE_EPOCH" mkfs.ext4 -q -F -d "$root" -L tqa-root -U "$FS_UUID" \
    -E "hash_seed=$FS_UUID,root_owner=0:0" -O ^has_journal -m 0 -b 4096 -I 256 \
    -N $((inodes + inodes / 5 + 1024)) "$image" "$((kib * 12 / 10 + 65536))k"
# The check's whole report is kept for diagnosis; the build's error names only its exit code (0 clean, 1 corrected, 4
# and above problems left; with -n nothing is corrected). The caller keeps the report beside its output.
code=0
e2fsck -fn "$image" >"$work/e2fsck.log" 2>&1 || code=$?
if [ "$code" -ne 0 ]; then
    printf '%s\n' "$code" >"$work/e2fsck.code"
    printf 'e2fsck exited %s on the new rootfs\n' "$code" >&2
    exit 3
fi
rm -f "$work/e2fsck.log"

# Hashes of what the guest will actually execute, read back from the built filesystem.
dump() {
    rm -f /tmp/dump
    debugfs -R "dump -p $1 /tmp/dump" "$image" 2>/dev/null
    sha256sum /tmp/dump | cut -d' ' -f1
}
runtime=$(dump /usr/local/bin/tqa-runtime)
init=$(dump /usr/sbin/tqa-vm-init)
# awk reads the whole output: under pipefail, `head -1` let a late second line die of SIGPIPE (141, SBX-F-036).
version=$(mke2fs -V 2>&1 | awk 'NR == 1 { print $2 }')
printf 'tree_sha256=%s\nruntime_sha256=%s\ninit_sha256=%s\ne2fsprogs=%s\n' \
    "$tree" "$runtime" "$init" "$version" >"$work/result.env"
