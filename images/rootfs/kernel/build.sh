#!/usr/bin/env bash
# Runs inside the kernel build stage: verify the pinned inputs, configure, build vmlinux, write the manifest.
set -euo pipefail
cd /src
printf '%s  %s\n' "$KERNEL_SHA256" linux.tar.xz "$CONFIG_SHA256" base.config | sha256sum -c --quiet
tar -xf linux.tar.xz
cd "linux-$KERNEL_VERSION"
cp ../base.config .config
scripts/kconfig/merge_config.sh -m .config ../tqa.config
make olddefconfig
# The fragment must survive olddefconfig; a silently dropped option would change the guest's posture.
for option in CONFIG_MODULES CONFIG_DEVTMPFS_MOUNT; do
    grep -qx "# $option is not set" .config
done
for option in CONFIG_IP_PNP CONFIG_CGROUP_PIDS CONFIG_MEMCG CONFIG_PID_NS CONFIG_SECCOMP_FILTER CONFIG_EXT4_FS \
    CONFIG_DEVTMPFS CONFIG_VIRTIO_BLK CONFIG_VIRTIO_NET CONFIG_SERIAL_8250_CONSOLE; do
    grep -qx "$option=y" .config
done
make -j"$JOBS" vmlinux
install -D -m 0644 vmlinux /out/vmlinux
install -m 0644 .config /out/config
sum() { sha256sum "$1" | cut -d' ' -f1; }
cat >/out/manifest.json <<JSON
{
  "schema": 1,
  "release": "$KERNEL_VERSION",
  "licence": "GPL-2.0-only",
  "source_sha256": "$KERNEL_SHA256",
  "base_config": "firecracker $FIRECRACKER_VERSION microvm-kernel-ci-x86_64-6.18.config",
  "base_config_sha256": "$CONFIG_SHA256",
  "fragment_sha256": "$(sum /src/tqa.config)",
  "config_sha256": "$(sum /out/config)",
  "vmlinux_sha256": "$(sum /out/vmlinux)"
}
JSON
