"""The Desktop's Chromium drop-in (ADR-0039 section 6): Debian's /usr/bin/chromium sources /etc/chromium.d/* before it
starts the browser, so every headed browser started by hand on the desktop (the window manager's Web browser,
x-www-browser, xterm, a link opened from an application) uses the runtime's profile in /run/tqa-browser, which every
run's end and Sign out everywhere empty; a browser that names its own profile, or a headless one, keeps its own. Run
with unittest discovery in this directory."""
from pathlib import Path
import re
import subprocess
import unittest

HERE = Path(__file__).resolve().parent
DROP_IN = HERE / "chromium-desktop-profile.sh"
PROFILE = "--user-data-dir=/run/tqa-browser/profile"


def flags(*arguments):
    """CHROMIUM_FLAGS after the drop-in is sourced with `arguments`, as Debian's wrapper sources it."""
    script = 'CHROMIUM_FLAGS="--existing"; . "$0"; printf "%s" "$CHROMIUM_FLAGS"'
    return subprocess.run(["sh", "-c", script.replace("$0", str(DROP_IN)), "chromium", *arguments],
                          capture_output=True, text=True, check=True).stdout


def desktop_stage(dockerfile):
    """The Dockerfile's Desktop files stage (desktop-os): from its FROM line to the next one."""
    return re.search(r"^FROM desktop-spike-os AS desktop-os\n(.*?)^FROM ", dockerfile,
                     re.MULTILINE | re.DOTALL).group(1)


class DesktopProfile(unittest.TestCase):
    def test_a_browser_started_by_hand_uses_the_runtimes_profile(self):
        for arguments in ((), ("https://example.com/",), ("--new-window", "file:///workspace/a.html")):
            self.assertEqual(flags(*arguments), "--existing " + PROFILE, arguments)

    def test_a_browser_naming_its_own_profile_or_headless_keeps_its_own(self):
        for arguments in (("--user-data-dir=/tmp/mine",), ("--headless=new", "--dump-dom", "https://example.com/"),
                          ("--headless", "about:blank"), ("--user-data-dir", "/tmp/mine")):
            self.assertEqual(flags(*arguments), "--existing", arguments)

    def test_only_the_desktop_image_installs_it_and_the_browser_alternatives_are_chromium(self):
        dockerfile = (HERE / "Dockerfile").read_text(encoding="utf-8")
        line = "COPY --chmod=0644 images/runtime/chromium-desktop-profile.sh /etc/chromium.d/tqa-desktop-profile"
        self.assertIn(line, desktop_stage(dockerfile))
        self.assertEqual(dockerfile.count(line), 1)
        stage = desktop_stage(dockerfile)
        for name in ("x-www-browser", "gnome-www-browser"):
            self.assertIn("update-alternatives --set {} /usr/bin/chromium".format(name), stage)
        allowed = (HERE / "Dockerfile.dockerignore").read_text(encoding="utf-8").splitlines()
        self.assertIn("!images/runtime/chromium-desktop-profile.sh", allowed)


if __name__ == "__main__":
    unittest.main()
