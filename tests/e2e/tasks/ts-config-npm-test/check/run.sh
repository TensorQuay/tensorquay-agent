# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
set -u
command -v npm >/dev/null || exit 2
rm -rf /tmp/p && cp -r /workspace /tmp/p && cd /tmp/p || exit 2
export HOME=/tmp/home npm_config_cache=/tmp/npm-cache npm_config_update_notifier=false
mkdir -p "$HOME"
if ! npm test >/tmp/npm.log 2>&1; then
    echo '{"passed": false, "reason": "npm test fails"}'
    tail -n 15 /tmp/npm.log
    exit 1
fi
# The summary line starts with "#" (TAP reporter) or an information sign (spec reporter, the default on Node 24).
if ! grep -Eq '^[^ ]+ tests 4$' /tmp/npm.log || ! grep -Eq '^[^ ]+ pass 4$' /tmp/npm.log; then
    echo '{"passed": false, "reason": "npm test does not run all four tests"}'
    grep -E '^[^ ]+ (tests|pass|fail) ' /tmp/npm.log
    exit 1
fi
echo '{"passed": true, "reason": "npm test runs all four tests and passes"}'
exit 0
