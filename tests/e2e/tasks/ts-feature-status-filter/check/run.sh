# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
set -u
command -v node >/dev/null || exit 2
cd /workspace || exit 2
if ! grep -rq 'status' test; then
    echo '{"passed": false, "reason": "no test covers the status filter"}'
    exit 1
fi
if node --test test/*.test.ts >/tmp/visible.log 2>&1 && node --test /check/hidden/*.test.ts >/tmp/hidden.log 2>&1; then
    echo '{"passed": true, "reason": "the status filter works alone and with tag, and unknown values throw"}'
    exit 0
fi
echo '{"passed": false, "reason": "a visible or held-out test fails"}'
grep -E '^not ok' /tmp/visible.log /tmp/hidden.log | head -n 10
exit 1
