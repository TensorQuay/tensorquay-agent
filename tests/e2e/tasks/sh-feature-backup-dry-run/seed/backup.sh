#!/bin/sh
# Copies every file under SOURCE into DEST, keeping relative paths, and reports each copy.
# Usage: sh backup.sh SOURCE DEST
set -eu
src=$1
dest=$2
find "$src" -type f | sort | while IFS= read -r file; do
    rel=${file#"$src"/}
    mkdir -p "$dest/$(dirname "$rel")"
    cp "$file" "$dest/$rel"
    printf 'copied %s\n' "$rel"
done
