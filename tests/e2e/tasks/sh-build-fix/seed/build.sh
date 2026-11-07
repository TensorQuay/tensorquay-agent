#!/bin/sh
# Builds dist/site.txt from the pages in src/, in page order, after a header line.
set -eu
out=dist/site.txt
printf 'Site build\n' > "$out"
for page in src/*.txt; do
    cat "$page" >> "$out"
done
