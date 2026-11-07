#!/usr/bin/env python3
"""Builds an environment image from a recipe (the built-in environments design, Step A), on this host's engine.

A recipe names a built-in base (minimal or workstation) and the packages to add from its own sources (apt, pip, npm).
The tool writes the Dockerfile itself, builds FROM the base's image ID, and labels the result so the gateway's image
list shows it: its name, its tools, the base it is built on, a digest of the recipe and the versions it actually got.
The image is tagged tqa-env:<name> and is never a built-in (those are the tqa-runtime:* tags). Pick it on the
Environments page to make an environment from it. Every refusal and build failure is one plain sentence.

    deploy/build-environment.sh recipe.toml [--timeout MINUTES] [--dry-run]
"""
import argparse
from dataclasses import dataclass, field
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile
import tomllib

BUILT_INS = ("minimal", "workstation", "data", "desktop")
BASES = {"minimal": "Minimal", "workstation": "Workstation"}
CAPABILITIES = ("shell", "python", "node", "build", "browser", "data")
FIELDS = {"name", "display_name", "base", "add", "capabilities"}
KINDS = ("apt", "pip", "npm")
# What each base can install: Minimal has apt only (no pip, no node).
INSTALLERS = {"minimal": {"apt"}, "workstation": {"apt", "pip", "npm"}}
# Package specs: a name and at most a pinned version, nothing that reads as an option, a path or a URL.
SPECS = {
    "apt": re.compile(r"[a-z0-9][a-z0-9+.-]*(=[A-Za-z0-9.+:~-]+)?"),
    "pip": re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*(\[[A-Za-z0-9,._-]+\])?(==[A-Za-z0-9.!+*_-]+)?"),
    "npm": re.compile(r"(@[a-z0-9][a-z0-9._-]*/)?[a-z0-9][a-z0-9._-]*(@[A-Za-z0-9.+_-]+)?"),
}
PINNED = {"apt": "=", "pip": "==", "npm": "@"}
KIND_WORDS = {"apt": "an apt", "pip": "a pip", "npm": "an npm"}
NAME = re.compile(r"[a-z][a-z0-9_-]{0,63}")
LABEL = "org.tensorquay.environment"
REVISION_LABEL = "org.tensorquay.protocol-revision"


class RecipeError(Exception):
    """A refusal or a build failure, as one plain sentence."""


@dataclass
class Recipe:
    name: str
    display_name: str
    base: str
    apt: list = field(default_factory=list)
    pip: list = field(default_factory=list)
    npm: list = field(default_factory=list)
    capabilities: list = field(default_factory=list)

    @property
    def pinned(self):
        """Every package names its version, so the same recipe builds the same packages."""
        return all(pinned(kind, spec) for kind in KINDS for spec in getattr(self, kind))


@dataclass
class Base:
    name: str
    image: str
    user: str
    capabilities: list


def pinned(kind, spec):
    rest = spec[1:] if kind == "npm" and spec.startswith("@") else spec
    return PINNED[kind] in rest


def parse(text):
    """The recipe in `text`, or RecipeError naming what isn't allowed."""
    try:
        document = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        line = re.search(r"line (\d+)", str(error))
        raise RecipeError("The recipe isn't valid TOML (line {}).".format(line.group(1) if line else "?")) from None
    add = document.get("add", {})
    for key in sorted(set(document) - FIELDS) + sorted(set(add) - set(KINDS) if isinstance(add, dict) else []):
        raise RecipeError("`{}` isn't a recipe field: a recipe lists packages only.".format(key))
    name, display, base = document.get("name"), document.get("display_name"), document.get("base")
    if not isinstance(name, str) or not NAME.fullmatch(name):
        raise RecipeError("The name must be lowercase letters, digits, - or _, starting with a letter.")
    if name in BUILT_INS:
        raise RecipeError("That name belongs to a built-in environment; choose another.")
    if not isinstance(display, str) or not 1 <= len(display) <= 64 or any(ord(c) < 32 for c in display):
        raise RecipeError("The display name must be 1 to 64 characters on one line.")
    if base not in BASES:
        raise RecipeError("The base must be minimal or workstation, a built-in environment.")
    packages = {}
    for kind in KINDS:
        specs = add.get(kind, [])
        if not isinstance(specs, list) or not all(isinstance(spec, str) for spec in specs):
            raise RecipeError("`{}` under [add] must be a list of package names.".format(kind))
        for spec in specs:
            if not SPECS[kind].fullmatch(spec):
                raise RecipeError("`{}` isn't {} package name: a recipe installs packages from the base's sources "
                                  "only.".format(spec, KIND_WORDS[kind]))
        packages[kind] = list(specs)
    if not any(packages.values()):
        raise RecipeError("The recipe adds no package: list at least one under [add].")
    capabilities = document.get("capabilities", [])
    for capability in capabilities if isinstance(capabilities, list) else [capabilities]:
        if capability not in CAPABILITIES:
            raise RecipeError("`{}` isn't a capability: use shell, python, node, build, browser or data.".format(capability))
    return Recipe(name=name, display_name=display, base=base, capabilities=list(capabilities), **packages)


def resolve_base(record, name, floor):
    """The built-in `name` from its image's inspect record (None when tqa-runtime:<name> isn't there): its tag, its
    environment label and its runtime revision must all agree, as for the gateway's built-ins."""
    display = BASES.get(name, name.capitalize())
    build = "deploy/build-images.sh" + (" --workstation" if name == "workstation" else "")
    if record is None:
        raise RecipeError("{} isn't on this host: build it with {} first.".format(display, build))
    labels = record.get("Config", {}).get("Labels") or {}
    if labels.get(LABEL) != name:
        raise RecipeError("tqa-runtime:{} isn't the {} environment (its label says {}): build it again with {}."
                          .format(name, display, labels.get(LABEL) or "nothing", build))
    try:
        revision = int(labels.get(REVISION_LABEL, ""))
    except ValueError:
        revision = -1
    if revision < floor:
        raise RecipeError("{} on this host is too old for this version: build it again with {}.".format(display, build))
    capabilities = [c for c in labels.get(LABEL + ".capabilities", "").split(",") if c]
    return Base(name=name, image=record["Id"], user=record["Config"].get("User") or "1000:1000",
                capabilities=capabilities)


def base_tag(base):
    """The tag the Dockerfile builds FROM: BuildKit can't take a local image ID there, so a tag made from the base's own
    ID stands for it, and check_start proves afterwards that the image starts from exactly that base."""
    return "tqa-env-base:{}-{}".format(base.name, base.image.split(":")[-1][:12])


def check_start(built_layers, base_layers, base):
    """None when the built image's layers begin with exactly the base's layers; otherwise the refusal."""
    if built_layers[:len(base_layers)] == base_layers and len(built_layers) >= len(base_layers):
        return None
    return "The build didn't start from {}'s image; nothing was tagged.".format(BASES[base.name])


def recipe_digest(recipe):
    """A digest of what the recipe asks for: order and layout don't change it; a name or a version does."""
    canonical = {"name": recipe.name, "display_name": recipe.display_name, "base": recipe.base,
                 "capabilities": sorted(recipe.capabilities), **{kind: sorted(getattr(recipe, kind)) for kind in KINDS}}
    return "sha256:" + hashlib.sha256(json.dumps(canonical, sort_keys=True).encode()).hexdigest()


def tag(recipe):
    return "tqa-env:" + recipe.name


def labels(recipe, base):
    capabilities = [c for c in CAPABILITIES if c in set(base.capabilities) | set(recipe.capabilities)]
    return {LABEL: recipe.name, LABEL + ".name": recipe.display_name, LABEL + ".capabilities": ",".join(capabilities),
            LABEL + ".base": recipe.base, LABEL + ".recipe": recipe_digest(recipe)}


def dockerfile(recipe, base):
    """The Dockerfile: FROM the base's image ID, the installs as root, the base's user back, then the labels."""
    for kind in KINDS:
        if getattr(recipe, kind) and kind not in INSTALLERS[base.name]:
            raise RecipeError("{} has no {}: build on Workstation, or list the tool under apt."
                              .format(BASES[base.name], kind))
    quoted = {kind: " ".join(shlex.quote(spec) for spec in getattr(recipe, kind)) for kind in KINDS}
    lines = ["# Written by deploy/build-environment.sh from the recipe {}; not for editing.".format(recipe.name),
             "FROM " + base_tag(base), "USER 0:0"]
    if recipe.apt:
        lines.append("RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "
                     + quoted["apt"] + " && rm -rf /var/lib/apt/lists/*")
    if recipe.pip:
        lines.append("RUN pip install --no-cache-dir --break-system-packages " + quoted["pip"])
    if recipe.npm:
        lines.append("RUN npm install -g --no-fund --no-audit " + quoted["npm"])
    lines.append("USER " + base.user)
    lines.append("LABEL " + " ".join("{}={}".format(key, json.dumps(value))
                                     for key, value in labels(recipe, base).items()))
    return "\n".join(lines) + "\n"


def explain(output, recipe, base):
    """A failed build's output as one plain sentence."""
    display = BASES[base.name]
    reach = "The build couldn't reach {0}. Check this host's network, then build again."
    # Causes first: an unreachable mirror only warns in apt-get update, and install then can't find the package.
    patterns = [
        (r"Temporary failure resolving '([^']+)'", reach),
        (r"Could not resolve '([^']+)'", reach),
        (r"Failed to establish a new connection.*?host='([^']+)'", reach),
        (r"ENOTFOUND (\S+)", reach),
        (r"[Nn]o space left on device", "The build ran out of disk space on this host. Free some space, then build again."),
        (r"Unable to locate package (\S+)", "The package {0} wasn't found in the apt sources of " + display + "."),
        (r"Version '([^']+)' for '([^']+)' was not found",
         "Version {0} of {1} isn't in the apt sources of " + display + " (any more): pin another version, or none."),
        (r"No matching distribution found for (\S+)", "The package {0} wasn't found on the Python package index."),
        (r"404 Not Found - GET https?://registry\.npmjs\.org/(\S+)", "The package {0} wasn't found on the npm registry."),
    ]
    for pattern, words in patterns:
        match = re.search(pattern, output, re.DOTALL)
        if match:
            return words.format(*(group.rstrip(".,") for group in match.groups()))
    return "The build failed; its last lines are above."


def timed_out(minutes):
    return "The build took longer than {} minute{} and was stopped.".format(minutes, "" if minutes == 1 else "s")


def resolved_label(versions):
    """The versions a build got, as the packages label: {"apt": {name: version}, "pip": ..., "npm": ...}."""
    return json.dumps({kind: dict(sorted(versions.get(kind, {}).items())) for kind in KINDS}, sort_keys=True,
                      separators=(",", ":"))


def drift(previous, current):
    """None when a rebuild got the versions the last build of the same recipe got; otherwise which changed."""
    before, after = json.loads(previous), json.loads(current)
    changed = ["{} {} {} -> {}".format(kind, name, before.get(kind, {}).get(name, "none"), version)
               for kind in KINDS for name, version in sorted(after.get(kind, {}).items())
               if before.get(kind, {}).get(name) != version]
    if not changed:
        return None
    return ("This build got other versions than the last build of this recipe: {}. Pin versions in the recipe to "
            "build the same image every time.".format(", ".join(changed)))


def revision_floor(root):
    """MIN_RUNTIME_REVISION from this checkout: the gateway refuses a runtime below it."""
    text = (Path(root) / "crates/tqa-types/src/limits.rs").read_text()
    return int(re.search(r"MIN_RUNTIME_REVISION: u16 = (\d+);", text).group(1))


def package_name(kind, spec):
    if kind == "npm":
        head, _, _ = spec[1:].partition("@") if spec.startswith("@") else spec.partition("@")
        return ("@" + head) if spec.startswith("@") else head
    return re.split(r"==|=|\[", spec)[0]


# The engine side: never in the unit tests (E2E-ENV-011 runs it on a real engine).
def docker(*arguments, timeout=120, check=True):
    return subprocess.run(["docker", *arguments], capture_output=True, text=True, timeout=timeout, check=check)


def inspect(reference):
    result = docker("image", "inspect", reference, check=False)
    return json.loads(result.stdout)[0] if result.returncode == 0 else None


def versions(image, recipe):
    """What the built image holds of each listed package, read inside it (no network)."""
    script = {
        "apt": "dpkg-query -W -f='${Package}\\t${Version}\\n' " + " ".join(
            shlex.quote(package_name("apt", s)) for s in recipe.apt),
        "pip": "python3 -m pip list --format=json 2>/dev/null",
        "npm": "npm ls -g --depth=0 --json 2>/dev/null",
    }
    got = {"apt": {}, "pip": {}, "npm": {}}
    for kind in KINDS:
        specs = getattr(recipe, kind)
        if not specs:
            continue
        output = docker("run", "--rm", "--network", "none", "--entrypoint", "sh", image, "-c", script[kind],
                        timeout=300).stdout
        wanted = {package_name(kind, s).lower().replace("_", "-") for s in specs}
        if kind == "apt":
            pairs = [line.split("\t") for line in output.splitlines() if "\t" in line]
        elif kind == "pip":
            pairs = [(entry["name"], entry["version"]) for entry in json.loads(output or "[]")]
        else:
            pairs = [(name, entry.get("version", "")) for name, entry in
                     json.loads(output or "{}").get("dependencies", {}).items()]
        got[kind] = {name: version for name, version in pairs if name.lower().replace("_", "-") in wanted}
    return got


def build(recipe_path, minutes, dry_run, root):
    recipe = parse(Path(recipe_path).read_text())
    repository = os.environ.get("TQA_RECIPE_BASE_REPOSITORY", "tqa-runtime")
    base = resolve_base(inspect("{}:{}".format(repository, recipe.base)), recipe.base, revision_floor(root))
    text = dockerfile(recipe, base)
    if dry_run:
        print(text, end="")
        return
    previous = inspect(tag(recipe))
    docker("tag", base.image, base_tag(base))
    try:
        built, packages = build_image(recipe, base, text, minutes)
    finally:
        docker("image", "rm", base_tag(base), check=False)
    report(recipe, previous, packages)


def build_image(recipe, base, text, minutes):
    """Builds, proves the base, records the versions it got, and tags tqa-env:<name>; returns the tag and versions."""
    with tempfile.TemporaryDirectory() as directory:
        Path(directory, "Dockerfile").write_text(text)
        print("== building {} from {} ({})".format(tag(recipe), BASES[recipe.base], base.image[:19]), flush=True)
        try:
            result = subprocess.run(["docker", "build", "--progress=plain", "-t", tag(recipe) + "-unlabelled",
                                     directory], capture_output=True, text=True, timeout=minutes * 60)
        except subprocess.TimeoutExpired:
            raise RecipeError(timed_out(minutes)) from None
        if result.returncode != 0:
            print("\n".join(result.stderr.splitlines()[-15:]), file=sys.stderr)
            raise RecipeError(explain(result.stderr + result.stdout, recipe, base))
        built = tag(recipe) + "-unlabelled"
        try:
            refused = check_start(inspect(built)["RootFS"]["Layers"], inspect(base.image)["RootFS"]["Layers"], base)
            if refused:
                raise RecipeError(refused)
            packages = resolved_label(versions(built, recipe))
            # The versions it got go in a label of their own: a last, empty layer on the build.
            Path(directory, "Dockerfile").write_text("FROM {}\nLABEL {}={}\n".format(
                built, LABEL + ".packages", json.dumps(packages)))
            docker("build", "-q", "-t", tag(recipe), directory, timeout=300)
        finally:
            # The unlabelled build already carries the environment labels: never leave it for the image list.
            docker("image", "rm", built, check=False)
    return tag(recipe), packages


def report(recipe, previous, packages):
    image = inspect(tag(recipe))
    print("== built {} ({}); pick it on the Environments page to make an environment from it".format(
        tag(recipe), image["Id"][:19]))
    print("   versions: {}".format(packages))
    if not recipe.pinned:
        print("   not every package names its version: a later build may get newer ones (the label records these)")
    previous_labels = (previous or {}).get("Config", {}).get("Labels") or {}
    if previous_labels.get(LABEL + ".recipe") == recipe_digest(recipe) and LABEL + ".packages" in previous_labels:
        message = drift(previous_labels[LABEL + ".packages"], packages)
        if message:
            print("   " + message)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("recipe", help="the recipe file (TOML)")
    parser.add_argument("--timeout", type=int, default=60, help="minutes the build may take (default 60)")
    parser.add_argument("--dry-run", action="store_true", help="print the Dockerfile and build nothing")
    arguments = parser.parse_args(argv)
    try:
        build(arguments.recipe, max(1, arguments.timeout), arguments.dry_run, Path(__file__).resolve().parents[2])
    except (RecipeError, OSError) as error:
        print("build-environment: {}".format(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
