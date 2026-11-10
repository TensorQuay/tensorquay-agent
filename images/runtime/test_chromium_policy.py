"""The browser's quiet setup (E2E-ENV-010, the Desktop's first minute): Chromium ignores a policy file it can't parse
without a word, and the background calls would come back, so the file must parse, hold exactly the policies chosen, and
reach the image; the switches file, which Debian's wrapper sources after its own files, must take Debian's remote
extensions back out and point the services no policy reaches at a closed loopback port. Run with unittest discovery in
this directory."""
from pathlib import Path
import json
import subprocess
import unittest

HERE = Path(__file__).resolve().parent
QUIET_POLICY = {
    "BrowserNetworkTimeQueriesEnabled": False,
    "DefaultSearchProviderEnabled": False,
    "BrowserSignin": 0,
    "SyncDisabled": True,
    "ComponentUpdatesEnabled": False,
    "BackgroundModeEnabled": False,
    "MetricsReportingEnabled": False,
    "UrlKeyedAnonymizedDataCollectionEnabled": False,
    "SafeBrowsingProtectionLevel": 0,
    "PasswordLeakDetectionEnabled": False,
    "SearchSuggestEnabled": False,
    "AlternateErrorPagesEnabled": False,
    "NetworkPredictionOptions": 2,
    "TranslateEnabled": False,
    "SpellCheckServiceEnabled": False,
    "SpellcheckEnabled": False,
    "EnableMediaRouter": False,
    "PromotionalTabsEnabled": False,
    "DnsOverHttpsMode": "off",
    # The browser runs with --no-sandbox (the task's sandbox is the boundary, computer/cdp.rs launch_args): without this
    # it shows "You are using an unsupported command-line flag … Stability and security will suffer." to the person
    # taking over and in every picture the model is sent (a pass over the pages before 17.0).
    "CommandLineFlagSecurityWarningsEnabled": False,
    # Real-world use before 17.0 (REL-I-004): on the Desktop the browser offered "Save password?" after a person's
    # sign-in and "Save address?" after a checkout form, in every picture the model was sent; the agent's sessions are
    # emptied at each run's end, so nothing it saves would last anyway.
    "PasswordManagerEnabled": False,
    "AutofillAddressEnabled": False,
    "AutofillCreditCardEnabled": False,
}
# Debian 13's /etc/chromium.d/default-flags and extensions, as they set CHROMIUM_FLAGS before ours is sourced.
DEBIAN_FLAGS = ("--show-component-extension-options --enable-gpu-rasterization --no-default-browser-check "
                "--disable-pings --media-router=0 --enable-remote-extensions --load-extension=")


class ChromiumPolicy(unittest.TestCase):
    def test_parses_and_holds_exactly_the_chosen_policies(self):
        policy = json.loads((HERE / "chromium-policy.json").read_text(encoding="utf-8"))
        self.assertEqual(policy, QUIET_POLICY)

    def test_the_image_installs_both_where_chromium_reads_them(self):
        dockerfile = (HERE / "Dockerfile").read_text(encoding="utf-8")
        self.assertIn(
            "COPY images/runtime/chromium-policy.json /etc/chromium/policies/managed/tqa-quiet.json",
            dockerfile,
        )
        # Sorted after Debian's own files (default-flags, extensions), so it sees and edits what they set.
        self.assertIn("COPY --chmod=0644 images/runtime/chromium-quiet-flags.sh /etc/chromium.d/tqa-quiet", dockerfile)
        self.assertGreater("tqa-quiet", "extensions")
        # The build context lists every file it lets in; both must be among them.
        allowed = (HERE / "Dockerfile.dockerignore").read_text(encoding="utf-8").splitlines()
        self.assertIn("!images/runtime/chromium-policy.json", allowed)
        self.assertIn("!images/runtime/chromium-quiet-flags.sh", allowed)


class QuietSwitches(unittest.TestCase):
    def flags(self, before):
        script = f'CHROMIUM_FLAGS="{before}"; . "{HERE / "chromium-quiet-flags.sh"}"; printf %s "$CHROMIUM_FLAGS"'
        return subprocess.run(["sh", "-c", script], check=True, capture_output=True, text=True).stdout.split()

    def test_remote_extensions_go_so_the_wrapper_turns_background_networking_off(self):
        flags = self.flags(DEBIAN_FLAGS)
        self.assertNotIn("--enable-remote-extensions", flags)
        # Debian's other flags stay as they were.
        for kept in DEBIAN_FLAGS.replace("--enable-remote-extensions ", "").split():
            self.assertIn(kept, flags)

    def test_the_services_no_policy_reaches_point_at_a_closed_loopback_port(self):
        flags = self.flags(DEBIAN_FLAGS)
        for switch in ("--gcm-checkin-url=http://127.0.0.1:9/checkin", "--gcm-mcs-endpoint=http://127.0.0.1:9/mcs",
                       "--component-updater=url-source=http://127.0.0.1:9/update", "--gaia-url=http://127.0.0.1:9/",
                       "--disable-component-update"):
            self.assertIn(switch, flags)
        features = next(flag for flag in flags if flag.startswith("--disable-features="))
        self.assertIn("OptimizationHintsFetching", features)
        # Autofill asks content-autofill.googleapis.com about each form's fields, whatever the policy says about saving:
        # a blocked notice in real-world use before 17.0 (REL-I-004).
        self.assertIn("AutofillServerCommunication", features.split("=", 1)[1].split(","))

    def test_without_debians_flags_it_adds_each_switch_once_and_still_drops_remote_extensions(self):
        once = self.flags("")
        self.assertEqual(once.count("--disable-component-update"), 1)
        self.assertNotIn("--enable-remote-extensions", self.flags(" --enable-remote-extensions"))


if __name__ == "__main__":
    unittest.main()
