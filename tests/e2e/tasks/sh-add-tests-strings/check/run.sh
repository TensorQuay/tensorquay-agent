# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
# The new tests must pass on lib/strings.sh and fail on each seeded mutant, so a test that asserts nothing cannot pass.
set -u
if [ ! -f /workspace/tests/test_strings.sh ]; then
    echo '{"passed": false, "reason": "tests/test_strings.sh is missing"}'
    exit 1
fi
if ! sh /workspace/tests/run.sh >/tmp/run.log 2>&1; then
    echo '{"passed": false, "reason": "the new tests fail on the real library"}'
    grep '^not ok' /tmp/run.log | head -n 10
    exit 1
fi
for mutant in /check/mutants/*.sh; do
    rm -rf /tmp/mutant && cp -r /workspace /tmp/mutant && cp "$mutant" /tmp/mutant/lib/strings.sh || exit 2
    if sh /tmp/mutant/tests/run.sh >/tmp/mutant.log 2>&1; then
        echo "{\"passed\": false, \"reason\": \"the tests do not catch $(basename "$mutant" .sh)\"}"
        exit 1
    fi
done
echo '{"passed": true, "reason": "the tests pass and catch every seeded mutant"}'
exit 0
