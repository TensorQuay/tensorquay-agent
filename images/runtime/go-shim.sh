#!/bin/sh
# Go builds, and for `go test` runs, binaries in GOTMPDIR. The sandbox's /tmp is small and mounted noexec, so GOTMPDIR
# defaults to the private home on the workspace, which exists only at run time; an explicit GOTMPDIR wins. The image
# does not set it, so a direct call of /usr/local/go/bin/go falls back to Go's own default. `go` on PATH is supported.
# The image declares no Go environment (the sandbox backend admits only the keys the gateway sets), so the shim also
# pins GOTOOLCHAIN=local: Go never downloads another toolchain, whatever a command sets.
set -eu
export GOTOOLCHAIN=local
export GOTMPDIR="${GOTMPDIR:-$HOME/.cache/go-tmp}"
mkdir -p "$GOTMPDIR"
exec /usr/local/go/bin/go "$@"
