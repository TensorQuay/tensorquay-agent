#!/bin/sh
# Runs every tests/test_*.sh with the library loaded. A test file calls `check NAME EXPECTED ACTUAL` for each
# assertion; the run fails when any check fails or when there are no checks at all.
cd "$(dirname "$0")/.." || exit 2
. lib/strings.sh
checks=0
failures=0
check() {
    checks=$((checks + 1))
    if [ "$2" = "$3" ]; then
        printf 'ok %s\n' "$1"
    else
        failures=$((failures + 1))
        printf 'not ok %s: expected [%s], got [%s]\n' "$1" "$2" "$3"
    fi
}
for file in tests/test_*.sh; do
    [ -f "$file" ] && . "./$file"
done
printf '%s checks, %s failed\n' "$checks" "$failures"
[ "$checks" -gt 0 ] && [ "$failures" -eq 0 ]
