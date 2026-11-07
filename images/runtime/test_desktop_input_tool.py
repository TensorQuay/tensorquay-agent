"""TOOL-U-064's image half (c16k real-world A10 and A12b, 17 Oct 2026): the model drove its desktop with xdotool from
the shell. The Desktop image keeps xdotool for the runtime's computer tool, off the sandbox user's PATH, and the tool
calls it there. REL-I-003 checks the built image."""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LIBEXEC = "/usr/libexec/tqa/xdotool"


class DesktopInputTool(unittest.TestCase):
    def test_the_desktop_stage_moves_xdotool_off_the_path(self):
        dockerfile = (ROOT / "images/runtime/Dockerfile").read_text()
        stage = dockerfile.split("FROM desktop-spike-os AS desktop-os", 1)[1].split("\nFROM ", 1)[0]
        # dpkg-divert, so dpkg (and the SBOM's C2) still know the file as xdotool's.
        self.assertIn("dpkg-divert --rename --divert " + LIBEXEC + " --add /usr/bin/xdotool", stage)

    def test_the_computer_tool_calls_it_there(self):
        x11 = (ROOT / "crates/tqa-tools/src/builtin/computer/x11.rs").read_text()
        self.assertRegex(x11, r'pub const XDOTOOL: &str = "{}";'.format(re.escape(LIBEXEC)))


if __name__ == "__main__":
    unittest.main()
