#!/usr/bin/env bash
# Regenerates the Linux screenshot baselines inside the official Playwright container (test tooling only; the image is
# never shipped). Font rasterisation differs by operating system, so Linux references must be produced on Linux.
#
#   scripts/linux-baselines.sh design
#       Rebuilds the test build and updates e2e/__screenshots__/linux/design-*.png against the fake front.
#   scripts/linux-baselines.sh ux STACK_URL TOKEN_FILE OUTPUT_DIR
#       Captures ux-* candidates from a stack started with `deploy/tests/demo.py --suite ux --keep` into OUTPUT_DIR;
#       copy them into e2e/__screenshots__/linux/ only after reviewing them.
#
# Run from web/. Linux dependencies, the package store and build output live in named volumes, never in web/.
# CI=true keeps pnpm non-interactive when it must rebuild the dependency volume.
# Node in the container resolves localhost to ::1 first; the fake front forwards pages to 127.0.0.1, so prefer IPv4.
set -euo pipefail

# Playwright 1.63.0 (the version pinned in package.json) on Ubuntu 24.04, Apache-2.0, pinned by digest.
readonly IMAGE="mcr.microsoft.com/playwright@sha256:eff16c30e6f3f4af0a03fa4b706120d5e9b0891c344a27d64559aff5900a4a27"
readonly VOLUME_PREFIX="tqa-linux-baselines"

mode="${1:-}"
repository="$(cd .. && pwd)"
[[ -f package.json && -d ../brand ]] || { echo "Run from web/ inside the repository" >&2; exit 2; }

run() {
  docker run --rm --init --ipc=host \
    --volume "$repository/web:/work/web" \
    --volume "$repository/brand:/work/brand:ro" \
    --volume "$repository/crates:/work/crates:ro" \
    --volume "$VOLUME_PREFIX-modules:/work/web/node_modules" \
    --volume "$VOLUME_PREFIX-next:/work/web/.next" \
    --volume "$VOLUME_PREFIX-store:/pnpm-store" \
    --env npm_config_store_dir=/pnpm-store \
    --env CI=true \
    --env NODE_OPTIONS=--dns-result-order=ipv4first \
    --workdir /work/web "$@"
}

readonly INSTALL="corepack enable >/dev/null && corepack pnpm install --frozen-lockfile >/dev/null"

case "$mode" in
  design)
    run "$IMAGE" bash -c "$INSTALL && corepack pnpm exec playwright test e2e/design.spec.ts --update-snapshots"
    ;;
  ux)
    [[ $# -eq 4 ]] || { echo "usage: $0 ux STACK_URL TOKEN_FILE OUTPUT_DIR" >&2; exit 2; }
    url="$2"
    token="$(sed -n 's/^TQA_BOOTSTRAP_TOKEN=//p' "$3")"
    output="$(mkdir -p "$4" && cd "$4" && pwd)"
    [[ -n "$token" ]] || { echo "No TQA_BOOTSTRAP_TOKEN in $3" >&2; exit 2; }
    run --volume "$output:/work/candidates" \
      --env TQA_STACK_URL="$url" --env TQA_STACK_SUITE=ux --env TQA_BOOTSTRAP_TOKEN="$token" \
      --env TQA_STACK_PASSWORD="synthetic-linux-$RANDOM$RANDOM$RANDOM" \
      --env TQA_UX_CAPTURE=1 --env TQA_WEB_EVIDENCE_DIR=/work/candidates \
      "$IMAGE" bash -c "$INSTALL && corepack pnpm exec playwright test --config playwright.stack.config.ts"
    ;;
  *)
    echo "usage: $0 design | ux STACK_URL TOKEN_FILE OUTPUT_DIR" >&2
    exit 2
    ;;
esac
