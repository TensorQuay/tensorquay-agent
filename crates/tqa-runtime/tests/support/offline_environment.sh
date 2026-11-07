#!/usr/bin/env bash
set -euo pipefail
test "$(id -u):$(id -g)" = 1000:1000
grep -Eq '^CapEff:[[:space:]]+0+$' /proc/self/status
grep -Eq '^NoNewPrivs:[[:space:]]+1$' /proc/self/status
grep -Eq '^Seccomp:[[:space:]]+2$' /proc/self/status
test "$HOME" = /workspace/.tqa/home
umask 077
mkdir -p "$HOME"
python3 - <<'PY'
import errno
import socket
with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
    probe.settimeout(1)
    try:
        probe.connect(('192.0.2.1', 443))
    except OSError as error:
        assert error.errno == errno.ENETUNREACH, error
    else:
        raise AssertionError('sandbox has a network route')
try:
    with open('/usr/tqa-write-probe', 'w') as output:
        output.write('unexpected')
except OSError as error:
    assert error.errno == errno.EROFS, error
else:
    raise AssertionError('root filesystem is writable')
PY
