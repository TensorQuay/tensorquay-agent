# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
# The new tests must pass on src/slug.ts and fail on each seeded mutant, so a test that asserts nothing cannot pass.
set -u
command -v node >/dev/null || exit 2
run() { (cd "$1" && node --test test/*.test.ts >/tmp/run.log 2>&1); }
if [ ! -f /workspace/test/slug.test.ts ]; then
    echo '{"passed": false, "reason": "test/slug.test.ts is missing"}'
    exit 1
fi
if ! run /workspace; then
    echo '{"passed": false, "reason": "the new tests fail on the real module"}'
    grep -E '^not ok' /tmp/run.log | head -n 10
    exit 1
fi
for mutant in /check/mutants/*.ts; do
    rm -rf /tmp/mutant && cp -r /workspace /tmp/mutant && cp "$mutant" /tmp/mutant/src/slug.ts || exit 2
    if run /tmp/mutant; then
        echo "{\"passed\": false, \"reason\": \"the tests do not catch $(basename "$mutant" .ts)\"}"
        exit 1
    fi
done
echo '{"passed": true, "reason": "the tests pass and catch every seeded mutant"}'
exit 0
