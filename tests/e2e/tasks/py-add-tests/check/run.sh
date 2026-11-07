# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
# The new tests must pass on stats.py and fail on each seeded mutant, so a test that asserts nothing cannot pass.
set -u
command -v python3 >/dev/null || exit 2
export PYTHONDONTWRITEBYTECODE=1
run() { (cd "$1" && python3 -m unittest discover -s tests -t . >/tmp/run.log 2>&1); }
if [ ! -f /workspace/tests/test_stats.py ]; then
    echo '{"passed": false, "reason": "tests/test_stats.py is missing"}'
    exit 1
fi
if ! run /workspace; then
    echo '{"passed": false, "reason": "the new tests fail on the real module"}'
    tail -n 15 /tmp/run.log
    exit 1
fi
for mutant in /check/mutants/*.py; do
    rm -rf /tmp/mutant && cp -r /workspace /tmp/mutant && cp "$mutant" /tmp/mutant/stats.py || exit 2
    if run /tmp/mutant; then
        echo "{\"passed\": false, \"reason\": \"the tests do not catch $(basename "$mutant" .py)\"}"
        exit 1
    fi
done
echo '{"passed": true, "reason": "the tests pass and catch every seeded mutant"}'
exit 0
