# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
# --throw-deprecation turns any remaining call of the legacy helper into a failure.
set -u
command -v node >/dev/null || exit 2
cd /workspace || exit 2
if grep -rln 'legacy/http' src/app >/tmp/left.log; then
    echo '{"passed": false, "reason": "an endpoint still imports the legacy helper"}'
    head -n 10 /tmp/left.log
    exit 1
fi
if node --throw-deprecation --test test/*.test.ts >/tmp/visible.log 2>&1 \
    && node --throw-deprecation --test /check/hidden/*.test.ts >/tmp/hidden.log 2>&1; then
    echo '{"passed": true, "reason": "every endpoint is migrated and returns the same line"}'
    exit 0
fi
echo '{"passed": false, "reason": "a visible or held-out test fails"}'
grep -E '^not ok|expected|actual' /tmp/visible.log /tmp/hidden.log | head -n 10
exit 1
