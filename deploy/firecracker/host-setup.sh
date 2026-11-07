#!/usr/bin/env bash
# Firecracker host setup (ADR-0006). The only root action: grant /dev/kvm to the VMM's mapped uid and gid, persistently,
# through a oneshot unit ordered after logind plus a timer that re-applies it within 30 seconds of a reset. The group
# entry is what makes the grant hold: logind's uaccess reset flushes every *user* ACL entry on each change event (one per
# VM created or destroyed), and a VM starting in that gap failed with EACCES; it never touches group entries.
# Usage: host-setup.sh [--check | --apply | --revert] [--user NAME]   (--check is the default and needs no root)
set -euo pipefail

VMM_CONTAINER_UID=10000   # Firecracker's uid and gid inside the per-VM container (images/vmm/launch.sh); never root.
TESTED_HOST_KERNELS="5.10 6.1 6.18"
UNITS=etc/systemd/system
LEGACY=etc/tmpfiles.d/tqa-kvm.conf  # Ineffective: logind resets the /dev/kvm ACL after tmpfiles runs.
ROOT="${TQA_HOST_ROOT:-}"  # A fake host root, for --check tests only.
mode=check user="" missing=0

die() { printf 'host-setup: %s\n' "$1" >&2; exit 2; }
ok() { printf 'ok      %s\n' "$1"; }; warn() { printf 'WARN %s\n' "$1"; }
need() { printf 'MISSING %s; remedy: %s\n' "$1" "$2"; missing=1; }

set_mode() { [[ $mode == check && -z ${mode_set:-} ]] || die "choose one mode"; mode="$1" mode_set=1; }
while (($#)); do
    case "$1" in
        --check) set_mode check ;;
        --apply) set_mode apply ;;
        --revert) set_mode revert ;;
        --user) (($# >= 2)) || die "--user needs a name"; user="$2"; shift ;;
        *) die "usage: host-setup.sh [--check | --apply | --revert] [--user NAME]" ;;
    esac
    shift
done

if [[ $mode != check ]]; then
    [[ -n $user ]] || die "--$mode needs --user NAME, the account that runs the rootless Docker daemon"
    [[ -z $ROOT ]] || die "TQA_HOST_ROOT is for --check only"
    [[ $(id -u) == 0 ]] || die "--$mode must run as root (sudo $0 --$mode --user $user)"
fi
user="${user:-$(id -un)}"
if [[ -n $ROOT ]]; then
    uid=$(awk -F: -v u="$user" '$1 == u { print $3 }' "$ROOT/etc/passwd")
else
    uid=$(getent passwd "$user" | cut -d: -f3) || true
fi
[[ $uid =~ ^[0-9]+$ ]] || die "unknown user: $user"

# Rootless Docker maps container uid N >= 1 to the user's subordinate range start + N - 1.
read -r sub_start sub_count < <(awk -F: -v u="$user" -v id="$uid" \
    '($1 == u || $1 == id) && !f { print $2, $3; f = 1 }' "$ROOT/etc/subuid" 2>/dev/null) || true
if [[ ${sub_count:-0} =~ ^[0-9]+$ ]] && ((${sub_count:-0} >= VMM_CONTAINER_UID)); then
    vmm_uid=$((sub_start + VMM_CONTAINER_UID - 1))
else
    [[ $mode == check ]] || die "no subordinate uid range of $VMM_CONTAINER_UID or more for $user"
    need "subordinate uid range of at least $VMM_CONTAINER_UID for $user" "configure /etc/subuid for rootless Docker"
fi
read -r gid_start gid_count < <(awk -F: -v u="$user" -v id="$uid" \
    '($1 == u || $1 == id) && !f { print $2, $3; f = 1 }' "$ROOT/etc/subgid" 2>/dev/null) || true
if [[ ${gid_count:-0} =~ ^[0-9]+$ ]] && ((${gid_count:-0} >= VMM_CONTAINER_UID)); then
    vmm_gid=$((gid_start + VMM_CONTAINER_UID - 1))
else
    [[ $mode == check ]] || die "no subordinate gid range of $VMM_CONTAINER_UID or more for $user"
    need "subordinate gid range of at least $VMM_CONTAINER_UID for $user" "configure /etc/subgid for rootless Docker"
fi
# logind resets device ACLs at seat setup, after tmpfiles and udev: grant after it, and re-apply on a timer.
unit() {
    if [[ $1 == service ]]; then printf '%s\n' "[Unit]" \
        "Description=TensorQuay Agent: grant /dev/kvm to the Firecracker VMM uid ${vmm_uid:-?} (ADR-0006)" \
        "After=systemd-logind.service display-manager.service systemd-udev-settle.service" \
        "ConditionPathExists=/dev/kvm" "" "[Service]" "Type=oneshot" "ExecStart=/usr/bin/setfacl -m u:${vmm_uid:-?}:rw,g:${vmm_gid:-?}:rw /dev/kvm"
    else printf '%s\n' "[Unit]" "Description=TensorQuay Agent: re-apply the /dev/kvm ACL for uid ${vmm_uid:-?} after a logind reset" \
        "" "[Timer]" "OnBootSec=1min" "OnUnitActiveSec=30s" "" "[Install]" "WantedBy=timers.target"; fi
}
if [[ $mode == apply ]]; then
    for kind in service timer; do install -m 0644 <(unit "$kind") "/$UNITS/tqa-kvm-acl.$kind"; done
    systemctl daemon-reload && systemctl enable --now tqa-kvm-acl.timer && systemctl start tqa-kvm-acl.service
    rm -f "/$LEGACY" && printf 'installed and enabled the tqa-kvm-acl units; granted /dev/kvm rw to uid %s and gid %s\n' "$vmm_uid" "$vmm_gid"
    exec "$0" --check --user "$user"
elif [[ $mode == revert ]]; then
    systemctl disable --now tqa-kvm-acl.timer 2>/dev/null || true
    rm -f "/$UNITS/tqa-kvm-acl.service" "/$UNITS/tqa-kvm-acl.timer" "/$LEGACY" && systemctl daemon-reload
    if [[ -e /dev/kvm ]]; then setfacl -x "u:$vmm_uid,g:$vmm_gid" /dev/kvm 2>/dev/null || true; fi
    printf 'removed the tqa-kvm-acl units; revoked /dev/kvm access for uid %s\n' "$vmm_uid" && exit 0
fi

if grep -Eqs '^flags.*\b(vmx|svm)\b' "$ROOT/proc/cpuinfo"; then ok "hardware virtualisation"
else need "hardware virtualisation (vmx or svm)" "enable VT-x or AMD-V in the firmware"; fi
if [[ -e $ROOT/dev/kvm ]]; then ok "/dev/kvm present"
else need "/dev/kvm" "load the kvm_intel or kvm_amd module"; fi
if [[ -n ${vmm_uid:-} ]]; then
    if getfacl -n -p "$ROOT/dev/kvm" 2>/dev/null | grep -qx "user:$vmm_uid:rw-"; then
        ok "/dev/kvm access for the VMM (u:$vmm_uid:rw)"
    else need "/dev/kvm access for uid $vmm_uid (container uid $VMM_CONTAINER_UID)" "sudo $0 --apply --user $user"; fi
    if [[ -n ${vmm_gid:-} ]] && getfacl -n -p "$ROOT/dev/kvm" 2>/dev/null | grep -qx "group:$vmm_gid:rw-"; then
        ok "/dev/kvm group access for the VMM (g:$vmm_gid:rw; logind's resets keep group entries)"
    elif [[ -n ${vmm_gid:-} ]]; then
        need "/dev/kvm group access for gid $vmm_gid (container gid $VMM_CONTAINER_UID; logind's resets remove the user entry)" \
            "sudo $0 --apply --user $user"
    fi
    for kind in service timer; do
        if cmp -s <(unit "$kind") "$ROOT/$UNITS/tqa-kvm-acl.$kind"; then ok "tqa-kvm-acl.$kind installed"
        else need "tqa-kvm-acl.$kind missing or different" "sudo $0 --apply --user $user"; fi
    done
    if [[ -L $ROOT/$UNITS/timers.target.wants/tqa-kvm-acl.timer ]] \
        && { [[ -n $ROOT ]] || systemctl is-active --quiet tqa-kvm-acl.timer; }; then ok "tqa-kvm-acl.timer enabled"
    else need "tqa-kvm-acl.timer enabled and active" "sudo $0 --apply --user $user"; fi
fi
if [[ -e $ROOT/$LEGACY ]]; then warn "/$LEGACY is ineffective (logind resets the ACL); --apply removes it"; fi
if [[ -e $ROOT/dev/net/tun ]]; then ok "/dev/net/tun present"
else need "/dev/net/tun" "load the tun module"; fi
controllers="$ROOT/sys/fs/cgroup/user.slice/user-$uid.slice/user@$uid.service/cgroup.controllers"
for controller in cpu memory pids; do
    if grep -qws "$controller" "$controllers"; then ok "cgroup controller $controller delegated"
    else need "cgroup controller $controller delegated to $user" "enable systemd user delegation (Delegate=cpu memory pids)"; fi
done
kernel=$(cut -d. -f1-2 "$ROOT/proc/sys/kernel/osrelease" 2>/dev/null || echo unknown)
if [[ " $TESTED_HOST_KERNELS " == *" $kernel "* ]]; then ok "host kernel $kernel"
else warn "host kernel $kernel is outside the Firecracker-tested hosts ($TESTED_HOST_KERNELS)"; fi
exit "$missing"
