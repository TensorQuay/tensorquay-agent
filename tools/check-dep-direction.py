#!/usr/bin/env python3
"""G-07: enforce workspace dependency direction and development-only test support."""

import argparse
from copy import deepcopy
import json
from pathlib import Path
import subprocess
import sys
import unittest


ALLOWED_EDGES = {
    "tqa-types": set(),
    "tqa-llm": {"tqa-types"},
    "tqa-context": {"tqa-types"},
    "tqa-tools": {"tqa-types"},
    "tqa-sandbox": {"tqa-types"},
    "tqa-prompt": {"tqa-types", "tqa-tools", "tqa-context"},
    "tqa-core": {"tqa-types", "tqa-llm", "tqa-context", "tqa-tools", "tqa-prompt"},
    "tqa-runtime": {"tqa-types", "tqa-llm", "tqa-context", "tqa-tools", "tqa-prompt", "tqa-core"},
    "tqa-gateway": {"tqa-types", "tqa-sandbox", "tqa-llm", "tqa-tools", "tqa-context"},
    "tqa-testkit": {"tqa-types", "tqa-llm", "tqa-context", "tqa-tools", "tqa-prompt"},
    "tqa-vm-init": set(),
    # Developer tooling (ext-check): reuses product code; nothing depends on it.
    "tqa-ext-check": {"tqa-types", "tqa-llm", "tqa-tools", "tqa-core", "tqa-runtime", "tqa-sandbox", "tqa-gateway"},
}


def violations(metadata):
    """Check normal and build edges, using package names even for renamed dependencies."""
    members = set(metadata["workspace_members"])
    packages = [package for package in metadata["packages"] if package["id"] in members]
    names = {package["name"] for package in packages}
    problems = []
    for package in packages:
        source = package["name"]
        if source not in ALLOWED_EDGES:
            problems.append(f"{source}: workspace crate is missing from the allowed edge table")
            continue
        for dependency in package["dependencies"]:
            target = dependency["name"]
            kind = dependency["kind"] or "normal"
            if kind == "dev":
                continue
            edge = f"{source} -> {target} ({kind})"
            if target == "tqa-testkit":
                problems.append(f"{edge}: tqa-testkit is dev-only")
            elif target in names and target not in ALLOWED_EDGES[source]:
                problems.append(f"{edge}: forbidden workspace dependency")
    return sorted(problems)


def run_check(root):
    """Read Cargo metadata, preserving diagnostics if Cargo rejects the graph itself."""
    try:
        result = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            cwd=root, capture_output=True, text=True, check=False,
        )
        if result.returncode:
            print("G-07: cargo metadata failed", file=sys.stderr)
            print(result.stderr.rstrip(), file=sys.stderr)
            return 1
        problems = violations(json.loads(result.stdout))
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"G-07: cannot read workspace dependencies: {error}", file=sys.stderr)
        return 1
    for problem in problems:
        print(problem)
    if not problems:
        print("G-07: workspace dependencies follow the allowed direction")
    return 1 if problems else 0


class DependencyDirectionTests(unittest.TestCase):
    """Check allowed edges and forbidden normal, build and test-support edges."""

    def setUp(self):
        self.graph = {
            "workspace_members": ["types", "llm", "gateway", "core", "testkit"],
            "packages": [
                {"id": "types", "name": "tqa-types", "dependencies": []},
                {"id": "llm", "name": "tqa-llm", "dependencies": [
                    {"name": "tqa-types", "kind": None},
                    {"name": "tqa-testkit", "kind": "dev"},
                ]},
                {"id": "gateway", "name": "tqa-gateway", "dependencies": [
                    {"name": "tqa-types", "kind": "build"},
                    {"name": "tqa-llm", "kind": None},
                ]},
                {"id": "core", "name": "tqa-core", "dependencies": [
                    {"name": "tqa-llm", "kind": None},
                ]},
                {"id": "testkit", "name": "tqa-testkit", "dependencies": [
                    {"name": "tqa-types", "kind": None},
                    {"name": "tqa-llm", "kind": None},
                ]},
            ],
        }

    def test_allowed_graph(self):
        self.assertEqual(violations(self.graph), [])

    def test_the_gateway_may_link_tools_for_the_shared_workspace_walker(self):
        # ADR-0030: the manifest walker and no-follow paths live in tqa-tools, shared with the runtime.
        graph = deepcopy(self.graph)
        graph["packages"][2]["dependencies"].append({"name": "tqa-tools", "kind": None})
        self.assertEqual(violations(graph), [])

    def test_forbidden_gateway_edge(self):
        for kind in (None, "build"):
            with self.subTest(kind=kind):
                graph = deepcopy(self.graph)
                graph["packages"][2]["dependencies"].append(
                    {"name": "tqa-core", "kind": kind, "rename": "engine"}
                )
                self.assertEqual(violations(graph), [
                    f"tqa-gateway -> tqa-core ({kind or 'normal'}): forbidden workspace dependency"
                ])

    def test_testkit_requires_development_dependency(self):
        for kind in (None, "build"):
            with self.subTest(kind=kind):
                graph = deepcopy(self.graph)
                graph["packages"][1]["dependencies"][1]["kind"] = kind
                self.assertEqual(violations(graph), [
                    f"tqa-llm -> tqa-testkit ({kind or 'normal'}): tqa-testkit is dev-only"
                ])


def main():
    """Run the gate or its standalone rule-engine tests."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(DependencyDirectionTests)
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1
    return run_check(Path(__file__).resolve().parent.parent)


if __name__ == "__main__":
    raise SystemExit(main())
