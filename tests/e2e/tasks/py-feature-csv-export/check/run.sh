# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
set -u
command -v python3 >/dev/null || exit 2
cd /workspace || exit 2
export PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=/workspace
if ! grep -rqi 'csv' tests; then
    echo '{"passed": false, "reason": "no test covers the CSV output"}'
    exit 1
fi
if python3 -m unittest discover -s tests -t . >/tmp/visible.log 2>&1 \
    && python3 -m unittest discover -s /check/hidden -t /check/hidden >/tmp/hidden.log 2>&1; then
    echo '{"passed": true, "reason": "text is unchanged, CSV is correct, other formats are rejected, tests pass"}'
    exit 0
fi
echo '{"passed": false, "reason": "a visible or held-out test fails"}'
tail -n 15 /tmp/visible.log /tmp/hidden.log 2>/dev/null
exit 1
