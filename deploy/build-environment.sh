#!/usr/bin/env bash
# Builds an environment image from a recipe on this host's engine, FROM a built-in (deploy/tools/build_environment.py;
# the admin guide's Environments section has a recipe and the steps). Usage: build-environment.sh recipe.toml [--dry-run]
exec python3 "$(dirname "$(readlink -f "$0")")/tools/build_environment.py" "$@"
