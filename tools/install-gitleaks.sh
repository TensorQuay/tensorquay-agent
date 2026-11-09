#!/usr/bin/env bash
# Install the MIT-licensed gate tool from a version- and checksum-pinned release.
set -euo pipefail

version=8.30.1
destination="${1:-$HOME/.local/bin}"
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)
        platform=linux_x64
        checksum=551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb
        ;;
    Darwin-x86_64)
        platform=darwin_x64
        checksum=dfe101a4db2255fc85120ac7f3d25e4342c3c20cf749f2c20a18081af1952709
        ;;
    Darwin-arm64)
        platform=darwin_arm64
        checksum=b40ab0ae55c505963e365f271a8d3846efbc170aa17f2607f13df610a9aeb6a5
        ;;
    *) printf 'Unsupported platform; install Gitleaks %s from its release assets.\n' "$version" >&2; exit 1 ;;
esac

download="$(mktemp -d)"
trap 'rm -rf -- "$download"' EXIT
archive="gitleaks_${version}_${platform}.tar.gz"
# A timeout or a 408, 429, 500, 502, 503 or 504 answer is tried again, up to 5 times with curl's back-off from 1 s,
# and no new try starts after 5 minutes. Any other error, a 404 among them, fails at once.
curl --fail --silent --show-error --location --connect-timeout 10 --max-time 120 --retry 5 --retry-max-time 300 \
    "https://github.com/gitleaks/gitleaks/releases/download/v${version}/${archive}" \
    --output "$download/$archive"
printf '%s  %s\n' "$checksum" "$download/$archive" | shasum --algorithm 256 --check
tar -xzf "$download/$archive" -C "$download" gitleaks LICENSE
mkdir -p "$destination"
install -m 0755 "$download/gitleaks" "$destination/gitleaks"
install -m 0644 "$download/LICENSE" "$destination/gitleaks.LICENSE"
"$destination/gitleaks" version
