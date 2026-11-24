#!/usr/bin/env bash
# Joins a published version's amd64 and arm64 images under the version, one index per image, and says each index's
# digest (.github/workflows/images.yml's publish job; REL-U-015). Usage: tools/release/join-images.sh VERSION, with
# IMAGES (the workflow's "name=local-tag" list) and REGISTRY in the environment; the caller is signed in to the registry.
# No annotations: the architecture images are Docker-format manifests, and the runner's Docker refuses annotations on
# an index of those (18.2.0's release run stopped here on 23 Nov 2026 after a runner update).
set -euo pipefail
version="${1:-}"
version="${version#v}"
# Exactly X.Y.Z, as the workflow's tag filter: a suffix would name an architecture's own tag.
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: tools/release/join-images.sh VERSION (for example 18.2.0)" >&2
  exit 2
fi
: "${IMAGES:?IMAGES is required}" "${REGISTRY:?REGISTRY is required}"
summary="${GITHUB_STEP_SUMMARY:-/dev/null}"
{
  echo "### Images of $version"
  echo
  echo "| Image | Index digest |"
  echo "|---|---|"
} >> "$summary"
for entry in $IMAGES; do
  name="${entry%%=*}"
  repository="$REGISTRY/tensorquay-agent-$name"
  docker buildx imagetools create -t "$repository:$version" "$repository:$version-amd64" "$repository:$version-arm64"
  digest="$(docker buildx imagetools inspect --format '{{.Manifest.Digest}}' "$repository:$version")"
  echo "$repository:$version $digest"
  echo "| \`$repository\` | \`$digest\` |" >> "$summary"
done
