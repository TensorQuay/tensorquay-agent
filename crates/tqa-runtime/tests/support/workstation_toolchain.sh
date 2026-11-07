#!/usr/bin/env bash
set -euo pipefail
python3 -m venv --system-site-packages .venv
cat > test_arithmetic.py <<'PY'
import sys
def test_addition():
    assert 19 + 23 == 42
def test_virtual_environment():
    assert sys.prefix != sys.base_prefix
PY
.venv/bin/python -m pip --version
.venv/bin/python -m pytest -q test_arithmetic.py
test "$(.venv/bin/python -m pip cache dir)" = "$HOME/.cache/pip"
node -e 'if (!process.release.lts || Number(process.versions.node.split(".")[0]) < 22) process.exit(1); console.log("node LTS: " + (19 + 23))'
corepack --version
npm init -y --offline
test "$(npm config get cache)" = "$HOME/.npm"
test -f package.json
git init --initial-branch=main
git add test_arithmetic.py
git -c user.name=TensorQuay -c user.email=admin@tensorquay.com commit -m 'test: verify offline workspace'
test "$(git rev-list --count HEAD)" = 1
cat > hello.c <<'C'
#include <stdio.h>
int main(void) { puts("hello toolchain"); return 0; }
C
gcc -Wall -Werror hello.c -o hello
test "$(./hello)" = 'hello toolchain'
g++ --version
make --version
rg --quiet 'test_addition' test_arithmetic.py
test "$(jq -r .name package.json)" = workspace
fdfind --quiet test_arithmetic.py
for tool in curl wget unzip zip less file ps; do command -v "$tool"; done
tree -J -L 1 | jq -e '.[0].type == "directory"'
test "$(sqlite3 sample.sqlite 'CREATE TABLE sample(value); INSERT INTO sample VALUES (42); SELECT value FROM sample;')" = 42
mkdir -p gosum && (
    cd gosum
    export GOPROXY=off GOFLAGS=-mod=mod
    go mod init example.com/gosum >/dev/null 2>&1
    cat > sum.go <<'GO'
package gosum

func Sum(values ...int) int {
	total := 0
	for _, value := range values {
		total += value
	}
	return total
}
GO
    cat > sum_test.go <<'GO'
package gosum

import "testing"

func TestSum(t *testing.T) {
	if got := Sum(19, 23); got != 42 {
		t.Fatalf("Sum = %d", got)
	}
}
GO
    go test ./... >/dev/null
    test "$(go env GOTOOLCHAIN)" = local
    # The shim pins it; the image declares only PATH and HOME (RT-I-022), and a command cannot unpin it.
    test -z "${GOTOOLCHAIN:-}"
    test "$(GOTOOLCHAIN=auto go env GOTOOLCHAIN)" = local
    gofmt -l . >/dev/null
    test "$(go env GOMODCACHE)" = "$HOME/go/pkg/mod"
    test "$(go env GOCACHE)" = "$HOME/.cache/go-build"
    test -d "$HOME/.cache/go-build"
    test "$(go env GOTMPDIR)" = "$HOME/.cache/go-tmp"
    # The shim derives GOTMPDIR from HOME; the image does not pin it, and an explicit value wins.
    test -z "${GOTMPDIR:-}"
    GOTMPDIR="$HOME/.cache/go-tmp-own" go env GOTMPDIR | grep -qx "$HOME/.cache/go-tmp-own"
    test -d "$HOME/.cache/go-tmp-own"
)
printf 'offline toolchain passed\n'
