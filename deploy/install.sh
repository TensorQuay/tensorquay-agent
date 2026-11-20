#!/usr/bin/env bash
# The one-command install: run from a release checkout, it asks where people will open TensorQuay Agent, which model
# server to use (or none, for a first try) and who the first administrator is; then it pulls the release's images,
# writes what the install guide's steps 1 and 2 make by hand, starts the stack and creates that administrator.
# Usage: deploy/install.sh [--yes] [--fake-model] [--help for every option]
# This wrapper checks Python and the Docker versions (the guide's two commands), then hands over to
# deploy/tools/install.py. The guide (deploy/README.md) keeps every step by hand.
set -euo pipefail

MINIMUM_ENGINE=27.4.0
MINIMUM_COMPOSE=2.31.0
REQUIREMENTS="Docker Engine $MINIMUM_ENGINE or newer and Docker Compose $MINIMUM_COMPOSE or newer are required (minimum tested versions)."

cd "$(dirname "$0")/.."

if ! command -v python3 > /dev/null; then
  echo "Python 3 is required (on a Mac, run python3 once and accept Apple's Command Line Tools)." >&2
  exit 2
fi
for arg in "$@"; do
  case "$arg" in
    -h|--help) exec python3 deploy/tools/install.py --help ;;
  esac
done

# Whether version $1 is at least $2 (dotted numbers; a suffix such as -rc1 or +ds is ignored).
at_least() {
  local have want i
  IFS=. read -r -a have <<< "${1%%[-+]*}"
  IFS=. read -r -a want <<< "$2"
  for i in 0 1 2; do
    if (( 10#${have[i]:-0} != 10#${want[i]:-0} )); then
      (( 10#${have[i]:-0} > 10#${want[i]:-0} ))
      return
    fi
  done
}

if ! command -v docker > /dev/null; then
  echo "Docker isn't installed. $REQUIREMENTS" >&2
  exit 2
fi
if ! engine="$(docker version --format '{{.Server.Version}}' 2> /dev/null)"; then
  echo "Docker isn't answering: start Docker (on a Mac, Docker Desktop) and run this again." >&2
  exit 2
fi
compose="$(docker compose version --short 2> /dev/null || true)"
if ! [[ "$engine" =~ ^[0-9]+\.[0-9]+ && "$compose" =~ ^v?[0-9]+\.[0-9]+ ]] \
  || ! at_least "$engine" "$MINIMUM_ENGINE" || ! at_least "${compose#v}" "$MINIMUM_COMPOSE"; then
  echo "$REQUIREMENTS This computer has Docker Engine ${engine:-unknown} and Compose ${compose:-none}." >&2
  exit 2
fi

umask 077
exec python3 deploy/tools/install.py "$@"
