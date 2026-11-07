# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
set -u
fail() { printf '{"passed": false, "reason": "%s"}\n' "$1"; exit 1; }
rm -rf /tmp/t && mkdir -p /tmp/t/src/docs/notes /tmp/t/src/img || exit 2
printf 'a' > /tmp/t/src/a.txt; printf 'b' > '/tmp/t/src/docs/b file.txt'; printf 'c' > /tmp/t/src/docs/notes/c.md
printf 'd' > /tmp/t/src/img/d.bin
cd /workspace || exit 2
expected='docs/b file.txt
docs/notes/c.md
img/d.bin
a.txt'
expected=$(printf '%s\n' "$expected" | sort)
dry=$(sh backup.sh --dry-run /tmp/t/src /tmp/t/dest 2>/tmp/dry.err) || fail "the dry run fails"
[ ! -e /tmp/t/dest ] || fail "the dry run created something under DEST"
[ "$dry" = "$(printf '%s\n' "$expected" | sed 's/^/would copy /')" ] || fail "the dry run does not print one would copy line per file in order"
real=$(sh backup.sh /tmp/t/src /tmp/t/dest 2>/tmp/real.err) || fail "the real run fails"
[ "$real" = "$(printf '%s\n' "$expected" | sed 's/^/copied /')" ] || fail "the real run output changed"
diff -r /tmp/t/src /tmp/t/dest >/dev/null || fail "the real run no longer copies every file"
echo '{"passed": true, "reason": "the dry run reports and changes nothing; the real run is unchanged"}'
exit 0
