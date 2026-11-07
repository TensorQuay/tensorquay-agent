# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
set -u
cd /workspace || exit 2
if ! sh start.sh --check >/tmp/check.log 2>&1; then
    echo '{"passed": false, "reason": "sh start.sh --check still fails"}'
    cat /tmp/check.log
    exit 1
fi
value() { grep -E "^$1=" config/app.env | head -n 1 | cut -d= -f2-; }
if [ "$(value NAME)" != "ledger-api" ] || [ "$(value WORKERS)" != "4" ]; then
    echo '{"passed": false, "reason": "a value that was already valid changed"}'
    exit 1
fi
echo '{"passed": true, "reason": "the configuration follows the specification and valid values are kept"}'
exit 0
