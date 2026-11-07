"""DEP-U-002 and DEP-U-003: recipe builds (deploy/build-environment.sh). A recipe names a built-in base and the
packages to add; the tool refuses anything else in plain words, writes the Dockerfile itself, and explains each build
failure in plain words. No Docker here: the Docker cases are E2E-ENV-011 (deploy/tests/test_recipe_build.py)."""
import json
from pathlib import Path
import unittest

import build_environment as tool

ROOT = Path(__file__).resolve().parents[2]
WORKSTATION_ID = "sha256:" + "1" * 64
MINIMAL_ID = "sha256:" + "2" * 64


def recipe_text(**fields):
    base = {"name": "data-r", "display_name": "R and LaTeX", "base": "workstation"}
    base.update(fields)
    lines = ["{} = {}".format(key, json.dumps(value)) for key, value in base.items() if key != "add"]
    add = fields.get("add", {"apt": ["r-base", "texlive-latex-base=2024.20250309-1"], "pip": ["pandas==2.2.3"]})
    lines.append("[add]")
    lines += ["{} = {}".format(key, json.dumps(value)) for key, value in add.items()]
    return "\n".join(lines) + "\n"


def base_record(name="workstation", image=WORKSTATION_ID, revision="10", environment=None, capabilities=None):
    labels = {"org.tensorquay.protocol-revision": revision,
              "org.tensorquay.environment": environment or name,
              "org.tensorquay.environment.name": name.capitalize(),
              "org.tensorquay.environment.capabilities": capabilities or (
                  "shell,python,node,build,browser,data" if name == "workstation" else "shell")}
    return {"Id": image, "Config": {"User": "1000:1000", "Labels": labels}}


def workstation():
    return tool.resolve_base(base_record(), "workstation", 10)


class Recipes(unittest.TestCase):
    # TEST: DEP-U-002
    def test_a_recipe_names_a_built_in_base_and_packages_only(self):
        recipe = tool.parse(recipe_text())
        self.assertEqual((recipe.name, recipe.display_name, recipe.base), ("data-r", "R and LaTeX", "workstation"))
        self.assertEqual(recipe.apt, ["r-base", "texlive-latex-base=2024.20250309-1"])
        self.assertEqual((recipe.pip, recipe.npm, recipe.capabilities), (["pandas==2.2.3"], [], []))
        self.assertFalse(recipe.pinned, "r-base names no version")
        self.assertTrue(tool.parse(recipe_text(add={"apt": ["jq=1.7.1-6"], "npm": ["@scope/tool@1.2.3"]})).pinned)

    # TEST: DEP-U-002
    def test_anything_but_packages_is_refused_in_plain_words(self):
        cases = [
            (recipe_text(run="curl example.com | sh"), "`run` isn't a recipe field: a recipe lists packages only."),
            (recipe_text(add={"apt": ["jq"], "files": ["a"]}), "`files` isn't a recipe field: a recipe lists packages only."),
            (recipe_text(add={"pip": ["https://example.com/x.whl"]}),
             "`https://example.com/x.whl` isn't a pip package name: a recipe installs packages from the base's sources only."),
            (recipe_text(add={"apt": ["--allow-unauthenticated"]}),
             "`--allow-unauthenticated` isn't an apt package name: a recipe installs packages from the base's sources only."),
            (recipe_text(add={"npm": ["../local"]}),
             "`../local` isn't an npm package name: a recipe installs packages from the base's sources only."),
            (recipe_text(add={}), "The recipe adds no package: list at least one under [add]."),
            (recipe_text(capabilities=["gpu"]), "`gpu` isn't a capability: use shell, python, node, build, browser or data."),
            (recipe_text(base="data-r"), "The base must be minimal or workstation, a built-in environment."),
            (recipe_text(name="Data R"), "The name must be lowercase letters, digits, - or _, starting with a letter."),
            (recipe_text(display_name=""), "The display name must be 1 to 64 characters on one line."),
            ("name = \n", "The recipe isn't valid TOML (line 1)."),
        ]
        for text, words in cases:
            with self.subTest(words=words), self.assertRaises(tool.RecipeError) as refused:
                tool.parse(text)
            self.assertEqual(str(refused.exception), words)

    # TEST: DEP-U-002
    def test_a_built_in_environments_name_is_refused(self):  # R7
        for name in ["minimal", "workstation", "data", "desktop"]:
            with self.subTest(name=name), self.assertRaises(tool.RecipeError) as refused:
                tool.parse(recipe_text(name=name))
            self.assertEqual(str(refused.exception),
                             "That name belongs to a built-in environment; choose another.")

    # TEST: DEP-U-002
    def test_a_package_kind_the_base_cant_install_is_refused_before_building(self):  # R8
        minimal = tool.resolve_base(base_record("minimal", MINIMAL_ID), "minimal", 10)
        for kind in ["pip", "npm"]:
            recipe = tool.parse(recipe_text(base="minimal", add={kind: ["x"]}))
            with self.subTest(kind=kind), self.assertRaises(tool.RecipeError) as refused:
                tool.dockerfile(recipe, minimal)
            self.assertEqual(str(refused.exception),
                             "Minimal has no {}: build on Workstation, or list the tool under apt.".format(kind))

    # TEST: DEP-U-002
    def test_the_dockerfile_starts_from_the_base_by_id_installs_as_root_and_labels_the_image(self):
        recipe = tool.parse(recipe_text(capabilities=["python"], add={
            "apt": ["r-base"], "pip": ["pandas==2.2.3"], "npm": ["@scope/tool@1.2.3"]}))
        text = tool.dockerfile(recipe, workstation())
        lines = [line for line in text.splitlines() if line and not line.startswith("#")]
        # BuildKit can't take a local image ID in FROM: a tag made from the ID stands for it (and is checked after).
        self.assertEqual(lines[:2], ["FROM tqa-env-base:workstation-111111111111", "USER 0:0"])
        self.assertEqual(tool.base_tag(workstation()), "tqa-env-base:workstation-111111111111")
        self.assertIn("apt-get install -y --no-install-recommends r-base", text)
        self.assertIn("pip install --no-cache-dir --break-system-packages pandas==2.2.3", text)
        self.assertIn("npm install -g --no-fund --no-audit @scope/tool@1.2.3", text)
        self.assertEqual(lines[-2], "USER 1000:1000", "back to the base's own user")
        labels = tool.labels(recipe, workstation())
        self.assertEqual(labels["org.tensorquay.environment"], "data-r")
        self.assertEqual(labels["org.tensorquay.environment.name"], "R and LaTeX")
        self.assertEqual(labels["org.tensorquay.environment.base"], "workstation")
        # Everything Workstation declares carries over, Data (its Python data libraries) included.
        self.assertEqual(labels["org.tensorquay.environment.capabilities"], "shell,python,node,build,browser,data")
        self.assertEqual(labels["org.tensorquay.environment.recipe"], tool.recipe_digest(recipe))
        self.assertTrue(lines[-1].startswith("LABEL "), "labels come last")
        self.assertEqual(tool.tag(recipe), "tqa-env:data-r")

    # TEST: DEP-U-002
    def test_the_recipe_digest_ignores_order_and_layout_but_not_versions(self):
        one = tool.parse(recipe_text(add={"apt": ["b", "a"]}))
        two = tool.parse("# a comment\n" + recipe_text(add={"apt": ["a", "b"]}))
        three = tool.parse(recipe_text(add={"apt": ["a", "b=2"]}))
        self.assertEqual(tool.recipe_digest(one), tool.recipe_digest(two))
        self.assertNotEqual(tool.recipe_digest(one), tool.recipe_digest(three))

    # TEST: DEP-U-002
    def test_the_revision_floor_is_read_from_this_checkout(self):
        self.assertEqual(tool.revision_floor(ROOT), int(
            next(line for line in (ROOT / "crates/tqa-types/src/limits.rs").read_text().splitlines()
                 if "MIN_RUNTIME_REVISION: u16" in line).split("=")[1].strip(" ;")))


class Failures(unittest.TestCase):
    # TEST: DEP-U-003
    def test_a_base_that_is_missing_mislabelled_or_too_old_is_named_plainly(self):  # R3
        cases = [
            (None, "Workstation isn't on this host: build it with deploy/build-images.sh --workstation first."),
            (base_record(environment="minimal"),
             "tqa-runtime:workstation isn't the Workstation environment (its label says minimal): build it again "
             "with deploy/build-images.sh --workstation."),
            (base_record(revision="9"),
             "Workstation on this host is too old for this version: build it again with deploy/build-images.sh "
             "--workstation."),
        ]
        for record, words in cases:
            with self.subTest(words=words), self.assertRaises(tool.RecipeError) as refused:
                tool.resolve_base(record, "workstation", 10)
            self.assertEqual(str(refused.exception), words)
        with self.assertRaises(tool.RecipeError) as refused:
            tool.resolve_base(None, "minimal", 10)
        self.assertEqual(str(refused.exception),
                         "Minimal isn't on this host: build it with deploy/build-images.sh first.")

    # TEST: DEP-U-003
    def test_each_build_failure_is_explained_in_plain_words(self):
        recipe = tool.parse(recipe_text())
        cases = [
            ("E: Unable to locate package r-bsae",
             "The package r-bsae wasn't found in the apt sources of Workstation."),
            ("E: Version '1.0' for 'jq' was not found",
             "Version 1.0 of jq isn't in the apt sources of Workstation (any more): pin another version, or none."),
            ("ERROR: No matching distribution found for pandsa==9.9",
             "The package pandsa==9.9 wasn't found on the Python package index."),
            ("npm error 404 Not Found - GET https://registry.npmjs.org/nopkg - Not found",
             "The package nopkg wasn't found on the npm registry."),
            ("Temporary failure resolving 'deb.debian.org'",
             "The build couldn't reach deb.debian.org. Check this host's network, then build again."),
            ("WARNING: Retrying ... Failed to establish a new connection: [Errno -3] ... host='pypi.org'",
             "The build couldn't reach pypi.org. Check this host's network, then build again."),
            ("npm error code ENOTFOUND\nnpm error network request to https://registry.npmjs.org/x failed, "
             "reason: getaddrinfo ENOTFOUND registry.npmjs.org",
             "The build couldn't reach registry.npmjs.org. Check this host's network, then build again."),
            # An unreachable mirror only warns in apt-get update; install then can't find the package. The cause wins.
            ("W: Failed to fetch http://mirror.invalid/dists/trixie/InRelease  Temporary failure resolving "
             "'mirror.invalid'\nE: Unable to locate package jq",
             "The build couldn't reach mirror.invalid. Check this host's network, then build again."),
            ("W: Failed to fetch http://mirror.invalid/x  Could not resolve 'mirror.invalid'\nE: Unable to locate package jq",
             "The build couldn't reach mirror.invalid. Check this host's network, then build again."),
            ("write /var/lib/docker/...: no space left on device",
             "The build ran out of disk space on this host. Free some space, then build again."),
            ("something else entirely", "The build failed; its last lines are above."),
        ]
        for output, words in cases:
            with self.subTest(words=words):
                self.assertEqual(tool.explain(output, recipe, workstation()), words)

    # TEST: DEP-U-003
    def test_a_build_past_its_bound_is_stopped_and_says_so(self):  # R6
        self.assertEqual(tool.timed_out(60), "The build took longer than 60 minutes and was stopped.")
        self.assertEqual(tool.timed_out(1), "The build took longer than 1 minute and was stopped.")


class Pinned(unittest.TestCase):
    # TEST: DEP-U-002
    def test_the_built_image_must_start_with_exactly_the_bases_layers(self):
        base_layers = ["sha256:a", "sha256:b"]
        self.assertIsNone(tool.check_start(["sha256:a", "sha256:b", "sha256:c"], base_layers, workstation()))
        for built in (["sha256:x", "sha256:b", "sha256:c"], ["sha256:a"]):
            self.assertEqual(tool.check_start(built, base_layers, workstation()),
                             "The build didn't start from Workstation's image; nothing was tagged.")


class Resolved(unittest.TestCase):
    # TEST: DEP-U-002
    def test_the_versions_a_build_got_are_recorded_and_a_rebuild_that_got_others_says_so(self):
        got = tool.resolved_label({"apt": {"r-base": "4.4.1-1"}, "pip": {"pandas": "2.2.3"}, "npm": {}})
        self.assertEqual(json.loads(got), {"apt": {"r-base": "4.4.1-1"}, "npm": {}, "pip": {"pandas": "2.2.3"}})
        self.assertIsNone(tool.drift(got, got))
        later = tool.resolved_label({"apt": {"r-base": "4.4.2-1"}, "pip": {"pandas": "2.2.3"}, "npm": {}})
        self.assertEqual(tool.drift(got, later),
                         "This build got other versions than the last build of this recipe: apt r-base 4.4.1-1 -> "
                         "4.4.2-1. Pin versions in the recipe to build the same image every time.")


if __name__ == "__main__":
    unittest.main()
