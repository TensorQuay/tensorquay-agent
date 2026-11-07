"""GW-I-063 (deployment): the skills overlay binds `[skills] root` into the gateway at the same path, read-only, and
refuses to render without it. The unit check reads the overlay itself (stdlib only, no engine: DEPLOY-U); rendering the
model through `docker compose config` runs only with TQA_DOCKER_TESTS=1, since that starts the engine's CLI."""

import json
import os
from pathlib import Path
import re
import subprocess
import unittest

REPOSITORY = Path(__file__).resolve().parents[2]
OVERLAY = REPOSITORY / "deploy" / "compose" / "skills.yml"
FILES = ["-f", "deploy/compose/docker.yml", "-f", "deploy/compose/skills.yml"]
# Placeholder values: the model is only rendered, never started.
ENV = {"TQA_INSTALL_DIR": "/srv/tqa-install", "TQA_STORAGE_ROOT": "/srv/tqa-storage",
       "TQA_POSTGRES_PASSWORD": "rendered-only", "TQA_BOOTSTRAP_TOKEN": "rendered-only"}
# `${NAME:?message}`: Compose refuses to render when NAME is unset or empty, and says the message.
REQUIRED_ROOT = re.compile(r"^\$\{TQA_SKILLS_ROOT:\?[^}]+\}$")


def overlay_volumes():
    """The gateway's volume entries in the overlay, each as its `key: value` lines (the file is plain block YAML)."""
    lines = [line for line in OVERLAY.read_text().splitlines() if line.strip() and not line.lstrip().startswith("#")]
    assert lines[:3] == ["services:", "  gateway:", "    volumes:"], lines[:3]
    entries = []
    for line in lines[3:]:
        assert line.startswith("      "), "Only the gateway's volumes belong in the overlay: " + line
        item = line[6:]
        if item.startswith("- "):
            entries.append({})
            item = item[2:]
        else:
            assert item.startswith("  "), line
            item = item[2:]
        key, _, value = item.partition(": ")
        entries[-1][key] = value
    return entries


class SkillsOverlay(unittest.TestCase):
    def test_the_overlay_binds_the_required_root_read_only_at_its_own_path(self):
        volumes = overlay_volumes()
        self.assertEqual(len(volumes), 1, volumes)
        volume = volumes[0]
        self.assertEqual(volume["type"], "bind")
        self.assertRegex(volume["source"], REQUIRED_ROOT)
        self.assertEqual(volume["target"], volume["source"])
        self.assertEqual(volume["read_only"], "true")
        self.assertEqual(volume["bind"], "{create_host_path: false}")


def render(extra):
    env = {"PATH": os.environ["PATH"], "HOME": os.environ.get("HOME", "/"), **ENV, **extra}
    return subprocess.run(["docker", "compose", *FILES, "config", "--format", "json"], cwd=REPOSITORY, env=env,
                          capture_output=True, text=True, timeout=60, check=False)


@unittest.skipUnless(os.environ.get("TQA_DOCKER_TESTS") == "1", "renders through the engine's CLI: TQA_DOCKER_TESTS=1")
class SkillsOverlayRendered(unittest.TestCase):
    def test_the_root_is_bound_read_only_at_its_own_path(self):
        result = render({"TQA_SKILLS_ROOT": "/srv/tqa-skills"})
        self.assertEqual(result.returncode, 0, result.stderr)
        volumes = json.loads(result.stdout)["services"]["gateway"]["volumes"]
        binds = [volume for volume in volumes if volume.get("target") == "/srv/tqa-skills"]
        self.assertEqual(len(binds), 1, volumes)
        self.assertEqual(binds[0]["type"], "bind")
        self.assertEqual(binds[0]["source"], "/srv/tqa-skills")
        self.assertIs(binds[0].get("read_only"), True)
        self.assertIs(binds[0]["bind"]["create_host_path"], False)

    def test_the_overlay_needs_the_root(self):
        result = render({})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("TQA_SKILLS_ROOT", result.stderr)


if __name__ == "__main__":
    unittest.main()
