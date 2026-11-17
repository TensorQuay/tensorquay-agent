#!/usr/bin/env bash
# G-05-chrono (ADR-0017 point 9, 17 Nov 2026): RUSTSEC-2026-0543 (chrono is unmaintained) is ignored only while no
# crate of ours depends on chrono; its direct users must stay oxidize-pdf and rmcp.
set -euo pipefail
users=$(cargo tree -q -i chrono -e normal,build,dev --depth 1 --prefix none | tail -n +2 | cut -d' ' -f1 | sort -u |
    tr '\n' ' ')
if [[ $users != "oxidize-pdf rmcp " ]]; then
    printf 'G-05-chrono: chrono is used by %s; only oxidize-pdf and rmcp may (ADR-0017 point 9)\n' \
        "${users:-nothing}" >&2
    exit 1
fi
echo "G-05-chrono: chrono's users are oxidize-pdf and rmcp only"
