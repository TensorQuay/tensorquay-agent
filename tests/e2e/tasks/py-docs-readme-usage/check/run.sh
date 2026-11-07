# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
set -u
command -v python3 >/dev/null || exit 2
cd /workspace || exit 2
export PYTHONDONTWRITEBYTECODE=1
COLUMNS=80 python3 find_lines.py --help > /tmp/help.txt 2>/dev/null || exit 2
python3 - <<'PY'
import sys
readme = open("README.md").read()
help_text = open("/tmp/help.txt").read()
head, marker, rest = readme.partition("## Usage\n\n```\n")
block, fence, tail = rest.partition("\n```\n")
if not marker or not fence:
    print('{"passed": false, "reason": "the Usage section or its fence is missing"}')
    sys.exit(1)
if block + "\n" != help_text:
    print('{"passed": false, "reason": "the Usage block differs from the command help"}')
    sys.exit(1)
if head != "# find_lines\n\nA small search tool.\n\n" or tail != "\n## Licence\n\nApache-2.0.\n":
    print('{"passed": false, "reason": "text outside the Usage block changed"}')
    sys.exit(1)
print('{"passed": true, "reason": "the Usage block matches the help and nothing else changed"}')
PY
