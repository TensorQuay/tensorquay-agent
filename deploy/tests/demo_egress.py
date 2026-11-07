"""SEC-001 (web side): the policy proxy refuses a fetch to a host outside a one-entry allow-list."""

import json
from pathlib import Path

import compose

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"

# The proxy port on the internal sandbox network; it is never published.
EGRESS_LISTEN = "0.0.0.0:3128"
# A reserved documentation name that the scenario never requests, so every fetch in it is refused.
ALLOWED_HOST = "allowed.example"


def configure(directory, env):
    """Enable the policy proxy with one unrelated allowed host; the scenario's host is therefore denied."""
    compose.ownership(directory, env)
    config = directory / "gateway.toml"
    rules = '\n[egress]\nlisten="{}"\n[[egress.allow]]\nhost="{}"\nports=[443]\n'.format(EGRESS_LISTEN, ALLOWED_HOST)
    compose.write(config, config.read_text() + rules)
    compose.ownership(directory, env, "1000:1000")


def verify_overlay(env, configuration):
    """The scripted model may read only the declared egress scenario."""
    config = json.loads(compose.command(configuration + ["config", "--format", "json"], env, capture=True).stdout)
    mounts = config["services"]["fake-model"].get("volumes", [])
    assert len(mounts) == 1, "Unexpected model fixture mounts"
    mount = mounts[0]
    assert mount.get("type") == "bind" and mount.get("read_only") is True, "The scenario mount must be read-only"
    assert mount.get("target") == "/scenarios/egress.json", "Unexpected scenario target"
    assert mount.get("source") == str((FIXTURES / "egress.json").resolve()), "Unexpected scenario source"


def verify_result(env):
    """The browser recorded the denied destination for one owned conversation."""
    result = json.loads(Path(env["TQA_STACK_RESULT"]).read_text())
    denials = result.get("denials")
    assert isinstance(denials, list) and denials, "The browser recorded no denied destination"
    for denial in denials:
        assert set(denial) == {"conversationId", "host", "port"}, "Unexpected denial record"
        assert denial["host"] == "example.invalid" and denial["port"] == 443, "Unexpected denied destination"
    print("Policy proxy: {} denied fetch(es) to example.invalid:443 observed live".format(len(denials)), flush=True)
