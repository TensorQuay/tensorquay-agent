"""SBX-F-036 (G-13-lifetime's rootfs build, 3 Oct 2026): a script under `pipefail` that keeps only the first line of a
command's output must read the whole output. `cmd | head -1` lets head exit after one line, so a command that writes
again afterwards dies of SIGPIPE: the pipeline is 141, and `set -e` ends the script with nothing on stderr."""

import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
MKROOTFS = ROOT / "images" / "rootfs" / "mkrootfs.sh"
# A first-line read through head: `| head -1`, `-n 1`, `-n1`, `--lines=1`.
FIRST_LINE_HEAD = re.compile(r"\|\s*head\s+(-1|-n\s*1|--lines=1)\b")
# A producer that writes its second line only after head has taken the first.
LATE_SECOND_LINE = "{ printf 'mke2fs 1.47.0 (5-Feb-2023)\\n'; sleep 0.2; printf '\\tUsing EXT2FS Library version 1.47.0\\n'; }"


def bash(script, env=None):
    return subprocess.run(["bash", "-c", script], capture_output=True, text=True, timeout=10, env=env, check=False)


class FirstLineUnderPipefail(unittest.TestCase):
    # TEST: SBX-F-036
    def test_head_dies_of_sigpipe_and_reading_the_whole_output_does_not(self):
        cut = bash(f"set -o pipefail; {LATE_SECOND_LINE} | head -1")
        self.assertEqual((cut.returncode, cut.stdout), (141, "mke2fs 1.47.0 (5-Feb-2023)\n"))
        whole = bash(f"set -o pipefail; {LATE_SECOND_LINE} | awk 'NR == 1'")
        self.assertEqual((whole.returncode, whole.stdout), (0, "mke2fs 1.47.0 (5-Feb-2023)\n"))

    # TEST: SBX-F-036
    def test_the_rootfs_builds_version_line_survives_a_late_second_line(self):
        line = next(l for l in MKROOTFS.read_text().splitlines() if l.startswith("version=$(mke2fs -V"))
        with tempfile.TemporaryDirectory() as bin_dir:
            fake = Path(bin_dir) / "mke2fs"
            fake.write_text(f"#!/bin/bash\n{LATE_SECOND_LINE}\n")
            fake.chmod(0o755)
            env = {**os.environ, "PATH": f"{bin_dir}:{os.environ['PATH']}"}
            run = bash(f"set -euo pipefail\n{line}\nprintf '%s' \"$version\"", env)
        self.assertEqual((run.returncode, run.stdout), (0, "1.47.0"), run.stderr)

    # TEST: SBX-F-036
    def test_no_pipefail_script_reads_a_first_line_through_head(self):
        offenders = []
        for top in ("images", "deploy", "tools"):
            for path in sorted((ROOT / top).rglob("*")):
                if not path.is_file() or "node_modules" in path.parts or path.suffix not in ("", ".sh"):
                    continue
                try:
                    text = path.read_text()
                except UnicodeDecodeError:
                    continue
                if "pipefail" not in text:
                    continue
                offenders += [f"{path.relative_to(ROOT)}:{n}" for n, l in enumerate(text.splitlines(), 1)
                              if FIRST_LINE_HEAD.search(l)]
        self.assertEqual(offenders, [], "read the whole output instead, e.g. awk 'NR == 1'")


if __name__ == "__main__":
    unittest.main()
