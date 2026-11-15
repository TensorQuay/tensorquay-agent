#!/usr/bin/env bash
# Pulls a release's prebuilt images (deploy/README.md, "0. Build or pull the images") from the GitHub Container
# Registry, pins each by the digest it resolves to, tags the runtimes as deploy/build-images.sh does, and prints the
# three `export` lines step 1 uses. Nothing is built or pushed. Each image is one index for amd64 and arm64, so Docker
# pulls the host's own architecture (arm64 on a Mac with Apple silicon).
# Usage: deploy/pull-images.sh [--workstation] [--desktop] [VERSION]
#   VERSION        the release, for example 18.0.0; by default the release tag this checkout is on
#   --workstation  also the Workstation environment (Python, Node, Go and a browser; about 3.0 GB)
#   --desktop      also the Desktop environment: the Workstation with a desktop the agent sees and operates (ADR-0039;
#                  pulls the Workstation too)
# TQA_IMAGE_REGISTRY names another registry or mirror holding the same names (default ghcr.io/tensorquay).
set -euo pipefail
cd "$(dirname "$0")/.."

usage="usage: deploy/pull-images.sh [--workstation] [--desktop] [VERSION]"
registry="${TQA_IMAGE_REGISTRY:-ghcr.io/tensorquay}"
workstation=0
desktop=0
version=""
for arg in "$@"; do
  case "$arg" in
    --workstation) workstation=1 ;;
    --desktop) desktop=1; workstation=1 ;;
    -*) echo "$usage" >&2; exit 2 ;;
    *)
      if [ -n "$version" ]; then echo "$usage" >&2; exit 2; fi
      version="${arg#v}"
      ;;
  esac
done
if [ -z "$version" ]; then
  tag="$(git describe --tags --exact-match 2>/dev/null || true)"
  version="${tag#v}"
fi
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
  echo "name the release to pull, for example: deploy/pull-images.sh 18.0.0 (this checkout is on no release tag)" >&2
  exit 2
fi

# pull NAME [LOCAL_TAG...]: pulls tensorquay-agent-NAME:VERSION, tags it locally, and sets `pinned` to its digest form.
pinned=""
pull() {
  local repository="$registry/tensorquay-agent-$1"
  shift
  echo "== $(date +%T) pulling $repository:$version"
  docker pull --quiet "$repository:$version" > /dev/null
  pinned="$(docker image inspect --format '{{range .RepoDigests}}{{println .}}{{end}}' "$repository:$version" \
    | grep -m 1 -F "$repository@sha256:" || true)"
  if [ -z "$pinned" ]; then
    echo "$repository:$version has no digest from the registry; it was not pulled from $registry" >&2
    exit 1
  fi
  local tag
  for tag in "$@"; do
    docker tag "$pinned" "$tag"
  done
}

# tqa-runtime:minimal, :workstation and :desktop are the canonical tags: the gateway registers the image each one names
# as that built-in environment (with its labels), exactly as for built images. :base stays for existing settings.
pull runtime tqa-runtime:base tqa-runtime:minimal
pull gateway
gateway="$pinned"
pull web
web="$pinned"
if [ "$workstation" = 1 ]; then
  pull workstation tqa-runtime:workstation
fi
if [ "$desktop" = 1 ]; then
  pull desktop tqa-runtime:desktop
fi

echo "== $(date +%T) done. For step 1:"
echo "export TQA_RUNTIME_IMAGE='$(docker image inspect --format '{{.Id}}' tqa-runtime:base)'"
echo "export TQA_GATEWAY_IMAGE='$gateway'"
echo "export TQA_WEB_IMAGE='$web'"
if [ "$workstation" = 1 ]; then
  echo "# Workstation is registered as an environment by itself at the gateway's next start (tqa-runtime:workstation)."
fi
if [ "$desktop" = 1 ]; then
  echo "# Desktop is registered the same way (tqa-runtime:desktop)."
fi
