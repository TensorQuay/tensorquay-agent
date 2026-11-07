# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
set -u
command -v node >/dev/null || exit 2
cd /workspace || exit 2
if [ ! -f src/validate.ts ] || grep -Eq 'function (isEmail|isHandle)' src/users.ts; then
    echo '{"passed": false, "reason": "the validators are not moved out of src/users.ts into src/validate.ts"}'
    exit 1
fi
if ! grep -Eq 'from "\./validate(\.ts)?"' src/users.ts; then
    echo '{"passed": false, "reason": "src/users.ts does not import from src/validate.ts"}'
    exit 1
fi
if node --test test/*.test.ts >/tmp/visible.log 2>&1 && node --test /check/hidden/*.test.ts >/tmp/hidden.log 2>&1; then
    echo '{"passed": true, "reason": "the validators moved, users import them and behaviour is unchanged"}'
    exit 0
fi
echo '{"passed": false, "reason": "a visible or held-out test fails"}'
grep -E '^not ok' /tmp/visible.log /tmp/hidden.log | head -n 10
exit 1
