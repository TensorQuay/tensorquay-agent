# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
set -u
command -v python3 >/dev/null || exit 2
cd /workspace || exit 2
export PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=/workspace
if grep -rn --include='*.py' 'calc_total' . >/tmp/grep.log; then
    echo '{"passed": false, "reason": "calc_total is still referenced"}'
    head -n 10 /tmp/grep.log
    exit 1
fi
if ! python3 -m unittest discover -s tests -t . >/tmp/visible.log 2>&1 \
    || ! python3 -m unittest discover -s /check/hidden -t /check/hidden >/tmp/hidden.log 2>&1; then
    echo '{"passed": false, "reason": "a visible or held-out test fails"}'
    tail -n 15 /tmp/visible.log /tmp/hidden.log 2>/dev/null
    exit 1
fi
if ! python3 -m shop.cli 2>/dev/null | cmp -s - /check/expected-invoice.txt; then
    echo '{"passed": false, "reason": "python3 -m shop.cli no longer prints the same invoice"}'
    exit 1
fi
echo '{"passed": true, "reason": "renamed everywhere, tests pass and the invoice is unchanged"}'
exit 0
