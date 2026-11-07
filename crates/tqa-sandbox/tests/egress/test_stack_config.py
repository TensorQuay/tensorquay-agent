"""TK-U-043 (candidate 14a's integration, 4 Oct 2026): the egress stack's config check judges the gateway's `check` by
its exit code, so an accepted file with a warning beside it (a deprecated key) passes and the warning is logged; and
the fixture authority is seeded in `[egress] ca_file`, not the deprecated `[git_push] ca_file`."""
import tempfile
import tomllib
import unittest
from pathlib import Path
from unittest import mock

from egress import stack

ACCEPTED = stack.CONFIG_ACCEPTED
WARNING = "WARN tqa_gateway::config: git_push.ca_file is deprecated; use egress.ca_file"


class ConfigCheckTests(unittest.TestCase):
    # TEST: TK-U-043
    def test_an_accepted_file_passes_with_or_without_warnings(self):
        self.assertIsNone(stack.config_refusal(0, ACCEPTED + "\n"))
        self.assertIsNone(stack.config_refusal(0, ACCEPTED + "\n" + WARNING + "\n"))
        self.assertEqual(stack.config_warnings(ACCEPTED + "\n" + WARNING + "\n\n"), [WARNING])
        self.assertEqual(stack.config_warnings(ACCEPTED), [])

    # TEST: TK-U-043
    def test_a_refusal_or_a_timeout_still_fails_with_the_output(self):
        refused = "tqa-gateway: invalid gateway configuration: unknown field `x`"
        self.assertIn(refused, stack.config_refusal(2, refused) or "")
        self.assertIn("exit 1", stack.config_refusal(1, "") or "")
        self.assertIn("timed out", stack.config_refusal(None, "") or "")
        # Exit 0 without the accepted line proves nothing: still refused (fail closed).
        self.assertIn("exit 0", stack.config_refusal(0, "") or "")
        self.assertIn("exit 0", stack.config_refusal(0, WARNING) or "")
        # The positive control is unchanged: only exit 2 with one configuration error proves the check refuses.
        self.assertIsNone(stack.control_refusal(2, refused))
        self.assertIsNotNone(stack.control_refusal(0, ACCEPTED))

    # TEST: TK-U-043
    def test_the_fixture_authority_goes_in_the_egress_table(self):
        parsed = tomllib.loads(stack.egress_table("192.0.2.9", "/storage/fixture-ca.pem"))
        self.assertEqual(parsed["egress"]["ca_file"], "/storage/fixture-ca.pem")
        self.assertEqual(parsed["egress"]["allow"], [{"host": "192.0.2.9", "ports": [8081, 8443]}])
        self.assertNotIn("git_push", parsed)
        # A case without an authority trusts none.
        self.assertNotIn("ca_file", tomllib.loads(stack.egress_table("192.0.2.9"))["egress"])

    # TEST: SBX-C-021
    def test_a_case_adds_its_public_hosts_after_the_upstream(self):
        parsed = tomllib.loads(stack.egress_table("192.0.2.9", extra=[("example.com", [443])]))
        self.assertEqual(parsed["egress"]["allow"], [{"host": "192.0.2.9", "ports": [8081, 8443]},
                                                     {"host": "example.com", "ports": [443]}])
        self.assertEqual(stack.Stack.extra_allow, ())


def listing(*names):
    return {"items": [{"name": name} for name in names]}


class StackWaitTests(unittest.TestCase):
    """TK-U-049 (c16's integration, 12 Oct 2026): a Desktop case waits for the built-in to be offered before naming
    it, and a failed case keeps the stack's and the sandboxes' logs before they are removed."""

    def owned(self, answers):
        owned = object.__new__(stack.Stack)
        owned.request = mock.Mock(side_effect=answers)
        return owned

    # TEST: TK-U-049
    def test_a_case_waits_until_the_environment_is_listed(self):
        owned = self.owned([listing("minimal"), listing("minimal", "workstation"), listing("desktop", "minimal")])
        owned.offered("desktop", bound=5, pause=0)
        self.assertEqual(owned.request.call_args_list, [mock.call("/api/v1/templates")] * 3)

    # TEST: TK-U-049
    def test_one_never_listed_fails_naming_what_is_offered(self):
        owned = self.owned(lambda _path: listing("minimal", "workstation"))
        with self.assertRaises(AssertionError) as raised:
            owned.offered("desktop", bound=0, pause=0)
        self.assertIn("desktop", str(raised.exception))
        self.assertIn("['minimal', 'workstation']", str(raised.exception))

    # TEST: TK-U-049
    def test_logs_are_kept_only_after_a_failure_and_never_after_ctrl_c(self):
        with mock.patch.object(stack.compose, "save_logs", return_value=[]) as save:
            self.assertEqual(stack.keep_logs({}, None, ["compose"]), [])
            self.assertEqual(stack.keep_logs({}, KeyboardInterrupt(), ["compose"]), [])
            save.assert_not_called()
            # Nothing written: the empty folder is removed.
            self.assertEqual(stack.keep_logs({"A": "1"}, AssertionError("x"), ["compose"]), [])
            folder = save.call_args.args[1]
            self.assertFalse(folder.exists())
            self.assertEqual(save.call_args.args[0], {"A": "1"})
            self.assertEqual(save.call_args.kwargs, {"configuration": ["compose"]})

    # TEST: TK-U-049
    def test_kept_logs_stay_and_their_folder_is_named(self):
        with tempfile.TemporaryDirectory() as kept:
            written = [Path(kept) / "sandbox-abc.log"]
            with mock.patch.object(stack.compose, "save_logs", return_value=written), \
                    mock.patch.object(stack.tempfile, "mkdtemp", return_value=kept), \
                    mock.patch("builtins.print") as said:
                self.assertEqual(stack.keep_logs({}, AssertionError("x"), ["compose"]), written)
            self.assertTrue(Path(kept).is_dir())
            said.assert_called_once_with("Stack logs kept in {}".format(kept), flush=True)


if __name__ == "__main__":
    unittest.main()
