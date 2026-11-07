# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
set -u
rm -rf /tmp/build && cp -r /workspace /tmp/build && cd /tmp/build || exit 2
rm -rf dist
if ! sh build.sh >/tmp/build.log 2>&1; then
    echo '{"passed": false, "reason": "sh build.sh fails"}'
    tail -n 15 /tmp/build.log
    exit 1
fi
if ! cmp -s dist/site.txt /check/expected.txt; then
    echo '{"passed": false, "reason": "dist/site.txt differs from the expected pages in numeric order"}'
    exit 1
fi
echo '{"passed": true, "reason": "the build succeeds from a clean tree and orders the pages numerically"}'
exit 0
