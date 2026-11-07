# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
set -u
cd /workspace || exit 2
fail() { printf '{"passed": false, "reason": "%s"}\n' "$1"; exit 1; }
grep -Eq '^[[:space:]]*deploy_to[[:space:]]*\(\)' deploy.sh || fail "deploy.sh defines no deploy_to function"
[ "$(grep -c 'sync build/ to' deploy.sh)" -eq 1 ] || fail "the per-environment block is still repeated"
for env in dev staging prod; do
    sh deploy.sh "$env" 2>/dev/null | cmp -s - "/check/expected-$env.txt" || fail "the output for $env changed"
done
set +e
sh deploy.sh nowhere >/tmp/out 2>/tmp/err
code=$?
set -e
[ "$code" -eq 2 ] && [ ! -s /tmp/out ] && grep -q "unknown environment 'nowhere'" /tmp/err || fail "an unknown environment no longer fails the same way"
echo '{"passed": true, "reason": "one deploy_to function, identical output and the same failure"}'
exit 0
