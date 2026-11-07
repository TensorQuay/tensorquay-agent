# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
# The last line carries the codeword stated only in the first message, so a pass shows it survived compaction.
set -u
cd /workspace || exit 2
if [ ! -f REPORT.md ]; then
    echo '{"passed": false, "reason": "REPORT.md is missing"}'
    exit 1
fi
if [ "$(tail -n 1 REPORT.md)" != "Audit reference: KESTREL-4471" ]; then
    echo '{"passed": false, "reason": "REPORT.md does not end with the audit reference from the first message"}'
    exit 1
fi
if ! cmp -s REPORT.md /check/expected.txt; then
    echo '{"passed": false, "reason": "REPORT.md does not list every part total in order"}'
    exit 1
fi
echo '{"passed": true, "reason": "every total is listed in order and the report ends with the first message codeword"}'
exit 0
