# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
set -u
command -v python3 >/dev/null || exit 2
cd /workspace || exit 2
export PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=/workspace
if python3 -m unittest discover -s /check/hidden -t /check/hidden >/tmp/hidden.log 2>&1; then
    echo '{"passed": true, "reason": "the crashing handler returns 0.0 and every handler behaves as before"}'
    exit 0
fi
echo '{"passed": false, "reason": "a held-out test fails"}'
tail -n 15 /tmp/hidden.log
exit 1
