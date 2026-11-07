#!/usr/bin/env bash
# VM host launcher (ADR-0006). It runs as the container's namespaced root only to create this VM's tap and load its
# rules, then becomes the fixed VMM uid with no capabilities, no_new_privs and an empty bounding set, and executes
# Firecracker. Everything it reads is a read-only file under /vm; nothing here reads from the guest.
set -euo pipefail

readonly VMM_UID=10000 TAP=tap0 TAP_ADDRESS=169.254.100.1/30
# Each step says so on stderr, and a failed one says which and how (139 is a segmentation fault): the gateway logs these
# lines when a VM host ends before its runtime registers, so a crash names where it happened.
step=files
trap 'status=$?; printf "launch: %s failed with status %s\n" "$step" "$status" >&2' ERR

for file in vm.json rules.nft vmlinux rootfs.img workspace.img state.img config.img exit.img; do
    [[ -e /vm/$file ]] || { printf 'launch: /vm/%s is missing\n' "$file" >&2; exit 64; }
done

step=tap
ip tuntap add dev "$TAP" mode tap user "$VMM_UID" group "$VMM_UID"
ip address add "$TAP_ADDRESS" dev "$TAP"
ip link set dev "$TAP" up
step=rules
nft -f /vm/rules.nft
printf 'launch: network ready, starting Firecracker\n' >&2

cd /
exec setpriv --reuid="$VMM_UID" --regid="$VMM_UID" --clear-groups \
    --inh-caps=-all --ambient-caps=-all --bounding-set=-all --no-new-privs \
    -- /usr/local/bin/firecracker --no-api --config-file /vm/vm.json
