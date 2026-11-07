#!/usr/bin/env bash
# Keep the Firecracker VMM's /dev/kvm grant (ADR-0006, owner decision (b)). Two udev rules:
# - 72 removes the uaccess tag from the device's current tags, so logind does not manage /dev/kvm afresh;
# - 74 re-grants the VMM in the same event. On systemd 255, TAG== matches the sticky TAGS, so the seat's uaccess
#   builtin (73-seat-late) still resets the ACL on every change event, and KVM sends one for each VM created or
#   destroyed. RUN programs run in rule order after all rules, so the re-grant follows the builtin.
# The re-grant runs host-setup.sh's own grant command from tqa-kvm-acl.service, whose timer remains the safety net.
# Cost: the seat (desktop or greeter) user no longer gets KVM automatically; grant it explicitly if it needs KVM.
# Usage: kvm-no-uaccess.sh [--check | --apply | --revert]   (--check is the default and needs no root)
set -euo pipefail

RULES=etc/udev/rules.d/72-tqa-kvm-no-uaccess.rules   # after 70-uaccess.rules sets the tag, before 73-seat-late applies it
REGRANT=etc/udev/rules.d/74-tqa-kvm-acl.rules         # after 73-seat-late schedules the uaccess builtin
UNIT=etc/systemd/system/tqa-kvm-acl.service           # installed by host-setup.sh --apply
ROOT="${TQA_HOST_ROOT:-}"  # A fake host root, for --check tests only.
UDEVADM="${TQA_UDEVADM:-}"  # A fake udevadm, for --check tests only.
mode=check missing=0

die() { printf 'kvm-no-uaccess: %s\n' "$1" >&2; exit 2; }
ok() { printf 'ok      %s\n' "$1"; }
need() { printf 'MISSING %s; remedy: %s\n' "$1" "$2"; missing=1; }
rule() {
    printf '%s\n' "# TensorQuay Agent (ADR-0006): logind must not manage /dev/kvm's ACL, so the Firecracker VMM's grant persists." \
        'SUBSYSTEM=="misc", KERNEL=="kvm", TAG-="uaccess"'
}
# The grant command from the unit, exactly; empty when the unit is absent or has no single grant.
grant() { sed -n 's|^ExecStart=\(/usr/bin/setfacl -m u:[0-9]*:rw\(,g:[0-9]*:rw\)\{0,1\} /dev/kvm\)$|\1|p' "$ROOT/$UNIT" \
    2>/dev/null | awk 'NR == 1' || true; }
regrant() {
    printf '%s\n' "# TensorQuay Agent (ADR-0006): re-grant the Firecracker VMM after the seat's uaccess builtin, in the same event." \
        "SUBSYSTEM==\"misc\", KERNEL==\"kvm\", ACTION!=\"remove\", RUN+=\"$1\""
}
reload() { udevadm control --reload && udevadm trigger --action=change --sysname-match=kvm && udevadm settle; }

while (($#)); do
    case "$1" in
        --check | --apply | --revert)
            [[ $mode == check && -z ${mode_set:-} ]] || die "choose one mode"
            mode="${1#--}" mode_set=1 ;;
        *) die "usage: kvm-no-uaccess.sh [--check | --apply | --revert]" ;;
    esac
    shift
done

if [[ $mode != check ]]; then
    [[ -z $ROOT && -z $UDEVADM ]] || die "TQA_HOST_ROOT and TQA_UDEVADM are for --check only"
    [[ $(id -u) == 0 ]] || die "--$mode must run as root (sudo $0 --$mode)"
fi

command=$(grant)
if [[ $mode == apply ]]; then
    [[ -n $command ]] || die "/$UNIT has no /dev/kvm grant; run host-setup.sh --apply first"
    install -m 0644 <(rule) "/$RULES" && install -m 0644 <(regrant "$command") "/$REGRANT"
    reload
    # Re-grant at once: an entry the builtin already removed returns only on the next event.
    systemctl start tqa-kvm-acl.service 2>/dev/null || true
    printf 'installed /%s and /%s\n' "$RULES" "$REGRANT"
    exec "$0" --check
elif [[ $mode == revert ]]; then
    rm -f "/$RULES" "/$REGRANT" && reload
    printf 'removed /%s and /%s; logind manages /dev/kvm again, and the timer alone restores the grant\n' "$RULES" "$REGRANT"
    exit 0
fi

if cmp -s <(rule) "$ROOT/$RULES"; then ok "/$RULES installed"
else need "/$RULES missing or different" "sudo $0 --apply"; fi
if [[ -z $command ]]; then
    need "tqa-kvm-acl.service with the VMM's /dev/kvm grant (/$UNIT)" "sudo host-setup.sh --apply --user <account>"
elif cmp -s <(regrant "$command") "$ROOT/$REGRANT"; then ok "/$REGRANT installed"
else need "/$REGRANT missing or different" "sudo $0 --apply"; fi
if [[ -n $UDEVADM ]] || { [[ -z $ROOT ]] && command -v udevadm >/dev/null; }; then
    if "${UDEVADM:-udevadm}" info -q property -n /dev/kvm 2>/dev/null | grep -Eq '^CURRENT_TAGS=.*:uaccess:'; then  # TAGS keeps past tags
        need "/dev/kvm still carries the uaccess tag" "sudo $0 --apply"
    else ok "/dev/kvm carries no uaccess tag"; fi
    # udevadm test prints the programs one change event would run, in order; it runs none of them.
    runs=$("${UDEVADM:-udevadm}" test --action=change /sys/class/misc/kvm 2>&1 | sed -n "s/^run: '\(.*\)'$/\1/p") || true
    last_builtin=$(grep -nx 'uaccess' <<<"$runs" | tail -1 | cut -d: -f1) || true
    last_grant=$(grep -nxF -- "${command:-/dev/kvm grant}" <<<"$runs" | tail -1 | cut -d: -f1) || true
    if [[ -n $last_grant ]] && ((last_grant > ${last_builtin:-0})); then ok "udev re-grants /dev/kvm after the uaccess builtin"
    else need "udev does not re-grant /dev/kvm after the uaccess builtin" "sudo $0 --apply"; fi
fi
exit "$missing"
