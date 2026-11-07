"""SEC-047 and SEC-049's microVM half (unit): the scripted runs give the guest hashes of the key's value, never the
value or any form of it, and each run ends with its own search and control. No Docker, no KVM."""

import ipaddress
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from egress import host_keys_microvm as microvm
from egress.host_keys import GIT_NAME, NAME

import guest_search  # on the path host_keys_microvm set
import compose
from egress import stack as egress_stack

VALUE = "tqa-g9-value-" + "5e" * 24
ADDRESS = "https://203.0.113.5:8443"


class Script(unittest.TestCase):
    def setUp(self):
        self.responses = microvm.responses(ADDRESS, ADDRESS[len("https://"):], VALUE)
        self.text = json.dumps(self.responses)

    def test_no_form_of_the_value_is_in_the_script_the_model_or_the_transcript_sees(self):
        for form in [VALUE] + guest_search.forms(VALUE):
            self.assertNotIn(form, self.text)

    def test_two_runs_each_end_with_a_search_for_every_form_and_a_fresh_control(self):
        commands = [response["events"][0]["arguments"]["command"] for response in self.responses
                    if response["events"][0]["type"] == "tool_call"]
        searches = [command for command in commands if guest_search.MARK in command]
        self.assertEqual(len(searches), 2)
        specs = [json.loads(command.split("SPEC = ")[1].split("\n")[0].strip()[1:-1].encode().decode("unicode_escape"))
                 for command in searches]
        for spec in specs:
            labels = [needle["label"] for needle in spec["needles"]]
            self.assertEqual(labels.count("control:hashed"), 1)
            self.assertEqual(len([label for label in labels if label.startswith("form:")]),
                             len(guest_search.forms(VALUE)))
        self.assertNotEqual(specs[0]["salt"], specs[1]["salt"], "each search has its own salt")
        texts = [response for response in self.responses if response["events"][0]["type"] == "text"]
        self.assertEqual(len(texts), 2, "two runs, each closed by an answer")
        self.assertTrue(all(response["when"] == microvm.PHRASE for response in self.responses))

    def test_the_first_run_calls_with_the_bearer_placeholder_and_the_second_pushes_with_git(self):
        commands = [response["events"][0]["arguments"].get("command", "") for response in self.responses
                    if response["events"][0]["type"] == "tool_call"]
        self.assertIn("${" + NAME + "}", commands[0])
        self.assertIn(ADDRESS + "/echo", commands[0])
        self.assertIn("git push", commands[3])
        self.assertIn("$" + GIT_NAME, commands[3])
        self.assertIn(ADDRESS + "/team/repo.git", commands[3])


class MicrovmNetwork(unittest.TestCase):
    """The microVM half draws its sandbox network as the microVM stack does (f2c83ac2), and pins that network's gateway
    address in gateway.toml; for 11 days it read a fixed subnet that no longer existed, unseen without KVM."""

    def test_the_network_is_drawn_first_and_its_gateway_pinned(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "stack"
            directory.mkdir()
            stack = egress_stack.Stack(directory, {})
            stack.backend = "firecracker"
            # The microVM stack's own networks, as it names them; the engine already has the first.
            networks = egress_stack.firecracker_stack().VM_NETWORKS
            taken = networks.format(0)
            with mock.patch.object(compose, "engine_subnets", return_value=[taken]):
                # No gateway image: prepare stops right after the draw, before anything touches Docker.
                with self.assertRaises(ValueError):
                    stack.prepare("host-keys")
            subnet = ipaddress.ip_network(stack.env["TQA_FC_SUBNET"])
            self.assertIn(str(subnet), [networks.format(third) for third in range(1, 256)])
            self.assertEqual(stack.gateway_address, stack.env["TQA_FC_GATEWAY_ADDRESS"])
            self.assertIn(ipaddress.ip_address(stack.gateway_address), subnet)
            (directory / "gateway.toml").write_text("[sandbox]\nbackend='docker'\n")
            stack.runtime = "sha256:" + "0" * 64
            stack.env["TQA_VMM_IMAGE"] = "tqa-vmm:test"
            with mock.patch.object(stack.microvm, "image_id", return_value="sha256:" + "1" * 64), \
                    mock.patch.object(stack.microvm, "artefacts"):
                stack.to_microvm()
            text = (directory / "gateway.toml").read_text()
            self.assertIn("backend='firecracker'", text)
            self.assertIn("gateway_address=" + json.dumps(stack.gateway_address), text)
            # Its sandboxes run the VM host image, which the sandbox image guard must count as under test.
            self.assertIn("sha256:" + "1" * 64, stack.env["TQA_PINNED_IMAGES"].split(","))


if __name__ == "__main__":
    unittest.main()
