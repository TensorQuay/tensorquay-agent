#!/usr/bin/env python3
"""List a phase gate's named test evidence offline, or explicitly run it with --run.

Use --only CORE,LLM,TOOL,RT for core evidence without the PERF-001 baseline.
Docker tests require TQA_DOCKER_TESTS=1 and their documented images; PostgreSQL
tests require equal TQA_DATABASE_URL and DATABASE_URL. Stack browser specs run
through their demo driver, which owns its stack and browser environment.
SBX-C-004 also requires TQA_PUBLIC_NETWORK_TESTS=1 and working public TCP/DNS.
Runtime recovery requires TQA_ALLOW_DEV_HOST_GATEWAY=1 and a Workstation image.
Requirements and default image selections are listed before any test runs.
Use --recorded ID=path/to/result.json[@SHA256] for an existing result instead of
running that ID: a soak result.json for SOAK-001...004 (status=passed,
gate_evidence=true, chaos_seconds>=7200 for Gate 2 or >=86400 for other gates), or
a TEST-PLAN 15.1 result file for any other ID (its id, environment, a clean tree
and verdict=pass). Recorded evidence is labelled with its path and SHA-256, not
rerun; a SHA-256 given after @ must match the file.
PERF-001 requires an idle host; --run uses release mode for that ID.
Direct target_os cfg mismatches are listed separately; each ID still needs a
passing test on this host. Other missing or disabled references fail.
Phase demos and other gate criteria remain part of lead acceptance.
"""

import argparse
from contextlib import redirect_stdout
from dataclasses import dataclass
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("check_docs", Path(__file__).with_name("check-docs.py"))
DOCS = importlib.util.module_from_spec(SPEC)
RECORDED_SPEC = importlib.util.spec_from_file_location("recorded_evidence",
                                                       Path(__file__).with_name("recorded_evidence.py"))
RECORDED = importlib.util.module_from_spec(RECORDED_SPEC)
RECORDED_SPEC.loader.exec_module(RECORDED)
RecordedEvidence = RECORDED.RecordedEvidence
SPEC.loader.exec_module(DOCS)
HOST_OS = {"darwin": "macos", "win32": "windows"}.get(sys.platform, sys.platform)
WEB_DEMO_SUITES = {
    "full-stack.spec.ts": ("basic", ()),
    "environment-stack.spec.ts": ("environment", ()),
    "approval-stack.spec.ts": ("approvals", ("WEB-U-003",)),
    "queue-stack.spec.ts": ("queue", ()),
    "egress-stack.spec.ts": ("egress", ("SEC-001",)),
    "ux-invariants.spec.ts": ("ux", ()),
}


@dataclass(frozen=True)
class Reference:
    path: str
    line: int
    name: str
    os_conditions: tuple = ()


def recorded_inputs(entries, selected):
    result = {}
    for entry in entries:
        identifier, artifact = RECORDED.parse(entry)
        if identifier not in selected:
            raise ValueError("--recorded names an ID this gate selection does not include: " + identifier)
        if identifier in result:
            raise ValueError("duplicate --recorded ID: " + identifier)
        result[identifier] = artifact
    return result


def report_recorded(identifier, artifact, gate, run):
    try:
        print(identifier + ": " + RECORDED.describe(identifier, artifact, gate), flush=True)
    except (OSError, UnicodeError, ValueError, RecursionError) as error:
        print(identifier + ": FAIL - recorded " + json.dumps(str(artifact.path)) + ": " + str(error), flush=True)
        return False
    if run:
        print(identifier + ": PASS (recorded)", flush=True)
    return True


def gate_ids(source, gate):
    definitions, _ = DOCS.test_plan(source)
    section = re.search(r"^## 17\. Phase gates\s*$(.*?)(?=^## |\Z)", source, re.M | re.S)
    if not section:
        raise ValueError("missing TEST-PLAN section 17")
    rows = [line.split("|")[3] for line in section[1].splitlines()
            if re.match(r"\|\s*Gate " + re.escape(gate) + r"\s*\|", line) and len(line.split("|")) >= 5]
    if len(rows) != 1 or not DOCS.expand_ids(rows[0]):
        raise ValueError("expected one nonempty Gate " + gate + " row")
    identifiers = list(dict.fromkeys(DOCS.expand_ids(rows[0])))
    missing = set(identifiers) - definitions.keys()
    if missing:
        raise ValueError("Gate " + gate + " names undefined IDs: " + ", ".join(sorted(missing)))
    return identifiers


def references(name, source):
    code, literals = list(source), {}
    for start, end, token in DOCS.source_tokens(source):
        code[start:end] = DOCS.blank(token)
        if not token.startswith(("//", "/*")):
            literals[start] = token
    code = "".join(code)
    declarations = list(test_declarations(name, source, code, literals))
    result = []
    for line, identifier in DOCS.source_references(name, source):
        same_line = [item for item in declarations
                     if source.count("\n", 0, item[0]) + 1 == line and identifier in item[3]]
        enclosing = sorted((item for item in declarations
                            if source.count("\n", 0, item[0]) + 1 < line <= source.count("\n", 0, item[1]) + 1),
                           key=lambda item: item[1] - item[0])
        following = [item for item in declarations if source.count("\n", 0, item[0]) + 1 >= line]
        selected = same_line or enclosing[:1] or following[:1]
        for start, _, title, _ in selected:
            conditions = os_conditions(source, code, start) if name.endswith(".rs") else ()
            result.append((identifier, Reference(name, line, title, conditions)))
        if not selected:
            result.append((identifier, Reference(name, line, "")))
    return list(dict.fromkeys(result))


def os_conditions(source, code, start):
    """Read only direct item attributes, never infer applicability from a test name."""
    attributes = re.search(r"((?:#\[[^\]]*\]\s*)+)(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?$", code[:start])
    if not attributes:
        return ()
    conditions = []
    for expression in re.findall(r"#\[\s*cfg\s*\((.*?)\)\s*\]", source[attributes.start(1):start], re.S):
        direct = re.fullmatch(r'\s*target_os\s*=\s*"([a-z0-9_]+)"\s*', expression)
        negated = re.fullmatch(r'\s*not\s*\(\s*target_os\s*=\s*"([a-z0-9_]+)"\s*\)\s*', expression)
        if direct or negated:
            conditions.append((bool(negated), (direct or negated)[1]))
    return tuple(conditions)


def os_exclusion(ref, host):
    for negated, target in ref.os_conditions:
        if (host == target) == negated:
            condition = 'target_os = "' + target + '"'
            if negated:
                condition = "not(" + condition + ")"
            return "not applicable on this host: cfg({}), host target_os = {}".format(condition, host)
    return ""


def test_declarations(name, source, code, literals):
    """Locate runnable names; all ID recognition remains owned by check-docs."""
    rust = Path(name).suffix == ".rs"
    expression = r"\bfn\s+(\w+)" if rust else r"\b(?:test|it|describe)(?:\.\w+)*\s*\("
    for match in re.finditer(expression, code):
        opening = code.find("(", match.end()) if rust else match.end() - 1
        if opening < 0:
            continue
        closing = DOCS.paired_end(code, opening, "(", ")")
        if closing is None and (rust or ".each" in match[0]):
            continue
        if rust:
            body = code.find("{", closing)
            end = DOCS.paired_end(code, body, "{", "}") if body >= 0 else None
            ids = [identifier for _, identifier in DOCS.source_references(name, "fn " + match[1] + "() {}")]
            yield match.start(), end or closing, match[1], ids
        else:
            if ".each" in match[0]:
                opening = closing + 1
                while opening < len(code) and code[opening].isspace():
                    opening += 1
                if opening == len(code) or code[opening] != "(":
                    continue
                closing = DOCS.paired_end(code, opening, "(", ")")
            title_start = opening + 1
            while title_start < len(source) and source[title_start].isspace():
                title_start += 1
            if title_start in literals:
                title = literals[title_start][1:-1]
                # A callback's regular expression must not erase its literal test title.
                end = closing if closing is not None else title_start + len(literals[title_start])
                yield title_start, end, title, DOCS.expand_ids(title)


def rust_target(ref):
    parts = Path(ref.path).parts
    if len(parts) < 4 or parts[0] != "crates":
        raise ValueError("unsupported Rust test location: " + ref.path)
    if parts[2] == "tests":
        return ["-p", parts[1], "--test", Path(parts[3]).stem]
    if parts[2:4] == ("src", "bin"):
        return ["-p", parts[1], "--bin", Path(parts[4]).stem]
    return ["-p", parts[1], "--lib"]


def rust_command(refs, release=False):
    expression = " or ".join("test(/(^|::)" + re.escape(ref.name) + "$/)" for ref in refs)
    return (["cargo", "nextest", "run"] + rust_target(refs[0]) + (["--release"] if release else [])
            + ["--run-ignored", "all", "--retries", "0", "--no-tests", "fail",
               "--ignore-default-filter", "-E", expression])


def check_inventory(data, refs):
    names = {name.split("::")[-1] for suite in data["rust-suites"].values() for name in suite["testcases"]}
    missing = sorted({ref.name for ref in refs} - names)
    if missing:
        raise ValueError("not runnable in nextest: " + ", ".join(missing))


def owned_web_suite(identifier, ref):
    if not ref.path.startswith("web/e2e/"):
        return None
    name = ref.path[len("web/e2e/"):]
    if name.endswith("-stack.spec.ts") and name not in WEB_DEMO_SUITES:
        raise ValueError("unmapped stack spec: " + ref.path)
    for suite, identifiers in WEB_DEMO_SUITES.values():
        if identifier in identifiers:
            return suite
    return WEB_DEMO_SUITES.get(name, (None, ()))[0]


def required_environment(refs, identifier):
    required = {"TQA_PUBLIC_NETWORK_TESTS": "1"} if identifier == "SBX-C-004" else {}
    for ref in refs:
        if (ref.path.startswith("crates/tqa-sandbox/tests/")
                or ref.path == "crates/tqa-runtime/tests/docker_image.rs"
                or owned_web_suite(identifier, ref) is not None):
            required["TQA_DOCKER_TESTS"] = "1"
        if ref.path == "crates/tqa-sandbox/tests/runtime_recovery.rs":
            required["TQA_ALLOW_DEV_HOST_GATEWAY"] = "1"
        if ref.path.startswith("crates/tqa-gateway/tests/") and not ref.path.endswith("/configuration.rs"):
            required.update(TQA_DATABASE_URL=None, DATABASE_URL=None)
    return required


def image_requirements(refs):
    defaults = {"crates/tqa-runtime/tests/docker_image.rs": "tqa-runtime:dev",
                "crates/tqa-sandbox/tests/runtime_recovery.rs": "tqa-runtime:workstation"}
    return sorted({"TQA_RUNTIME_TEST_IMAGE or built " + defaults[ref.path]
                   for ref in refs if ref.path in defaults})


def prerequisites(refs, environment, identifier=""):
    required = required_environment(refs, identifier)
    missing = sorted(key + ("=" + value if value else " (nonempty)") for key, value in required.items()
                     if not environment.get(key) or value is not None and environment[key] != value)
    if missing:
        raise ValueError(identifier + " requires " + ", ".join(missing))
    if "DATABASE_URL" in required and environment["DATABASE_URL"] != environment["TQA_DATABASE_URL"]:
        raise ValueError("DATABASE_URL must equal TQA_DATABASE_URL")
    if image_requirements(refs) and "TQA_RUNTIME_TEST_IMAGE" in environment and not environment["TQA_RUNTIME_TEST_IMAGE"].strip():
        raise ValueError("TQA_RUNTIME_TEST_IMAGE must be nonempty when set")


def environment_preflight(mapping, run):
    problems = {}
    for identifier, refs in mapping.items():
        if isinstance(refs, RecordedEvidence):
            continue
        applicable = [ref for ref in refs if not os_exclusion(ref, HOST_OS)]
        try:
            required = required_environment(applicable, identifier)
            notes = [key + ("=" + value if value else " (nonempty, equal database URLs)")
                     for key, value in sorted(required.items())] + image_requirements(applicable)
            if notes:
                print(identifier + " requires: " + "; ".join(notes), flush=True)
            if run:
                prerequisites(applicable, os.environ, identifier)
        except ValueError as error:
            problems[identifier] = str(error)
    return problems


def web_results(data, pattern, browser=False):
    if browser:
        results = []
        pending = list(data.get("suites", []))
        while pending:
            suite = pending.pop()
            pending.extend(suite.get("suites", []))
            for spec in suite.get("specs", []):
                if re.search(pattern, spec["title"]):
                    for test in spec["tests"]:
                        results.append(test.get("expectedStatus") == "passed"
                                       and bool(test.get("results"))
                                       and all(result["status"] == "passed" for result in test["results"]))
        return bool(results) and all(results)
    statuses = [item["status"] for file in data.get("testResults", []) for item in file["assertionResults"]
                if re.search(pattern, item["fullName"])]
    return bool(statuses) and all(status == "passed" for status in statuses)


def execute(command, root, output=None):
    print("$ " + " ".join(command), flush=True)
    result = subprocess.run(command, cwd=root, stdout=output, timeout=1800, check=False)
    if result.returncode:
        raise ValueError("runner exited " + str(result.returncode))


def run_rust(root, refs, release):
    target = rust_target(refs[0]) + (["--release"] if release else [])
    command = ["cargo", "nextest", "list"] + target + ["--ignore-default-filter", "--message-format", "json"]
    with tempfile.TemporaryFile(mode="w+") as output:
        execute(command, root, output)
        output.seek(0)
        check_inventory(json.load(output), refs)
    execute(rust_command(refs, release), root)


def run_web(root, ref):
    browser = ref.path.startswith("web/e2e/")
    pieces = re.split(r"(\$\{[^{}]+\})", ref.name)
    pattern = r"(?:^|\s)" + "".join(".*" if part.startswith("${") and part.endswith("}")
                                   else re.escape(part) for part in pieces)
    pattern += "$" if browser else r"(?:\s|$)"
    name = str(Path(ref.path).relative_to("web"))
    with tempfile.TemporaryDirectory() as directory:
        report = Path(directory) / "results.json"
        if browser:
            command = ["corepack", "pnpm", "exec", "playwright", "test", name, "--grep", pattern,
                       "--retries=0", "--reporter=json"]
            with report.open("w", encoding="utf-8") as output:
                execute(command, root / "web", output)
        else:
            command = ["corepack", "pnpm", "exec", "vitest", "run", name, "-t", pattern,
                       "--reporter=json", "--outputFile=" + str(report), "--retry=0"]
            execute(command, root / "web")
        if not web_results(json.loads(report.read_text(encoding="utf-8")), pattern, browser):
            raise ValueError("web runner did not pass every selected test (empty, skipped or failed)")


def run_id(root, identifier, refs):
    prerequisites(refs, os.environ, identifier)
    groups = {}
    demos_ran = set()
    for ref in refs:
        if not ref.name:
            raise ValueError("no runnable test name at {}:{}".format(ref.path, ref.line))
        if ref.path.endswith(".rs"):
            groups.setdefault(tuple(rust_target(ref)), []).append(ref)
        else:
            suite = owned_web_suite(identifier, ref)
            if suite is None:
                run_web(root, ref)
            elif suite not in demos_ran:
                command = ["python3", "deploy/tests/demo.py"]
                if suite != "basic":
                    command += ["--suite", suite]
                execute(command, root)
                demos_ran.add(suite)
    for group in groups.values():
        run_rust(root, group, identifier.startswith("PERF-"))


def evidence(root, gate, only, run, recorded=()):
    try:
        ids = gate_ids((root / "docs/TEST-PLAN.md").read_text(encoding="utf-8"), gate)
        areas = {identifier.split("-")[0] for identifier in ids}
        unknown = set(only) - set(ids) - areas
        if unknown:
            raise ValueError("unknown --only selection: " + ", ".join(sorted(unknown)))
        selected = [identifier for identifier in ids
                    if not only or identifier in only or identifier.split("-")[0] in only]
        mapping = {identifier: [] for identifier in selected}
        for name in DOCS.git_files(root):
            if Path(name).suffix in DOCS.SOURCE_SUFFIXES:
                for identifier, ref in references(name, (root / name).read_text(encoding="utf-8")):
                    if identifier in mapping:
                        mapping[identifier].append(ref)
        mapping.update(recorded_inputs(recorded, selected))
        return report_evidence(root, gate, ids, mapping, run)
    except (OSError, UnicodeError, ValueError, subprocess.SubprocessError) as error:
        print("Gate evidence: FAIL - " + str(error), flush=True)
        return 1


def report_evidence(root, gate, ids, mapping, run):
    failures, inapplicable = 0, 0
    recorded = 0
    preflight = environment_preflight(mapping, run)
    for identifier, refs in mapping.items():
        if isinstance(refs, RecordedEvidence):
            recorded += 1
            failures += not report_recorded(identifier, refs, gate, run)
            continue
        exclusions = {ref: os_exclusion(ref, HOST_OS) for ref in refs}
        applicable = [ref for ref in refs if not exclusions[ref]]
        inapplicable += len(refs) - len(applicable)
        names = "; ".join("{}:{} {}{}".format(ref.path, ref.line, ref.name or "no test",
                                            " [" + exclusions[ref] + "]" if exclusions[ref] else "") for ref in refs)
        print(identifier + ": " + (names or "no test"), flush=True)
        problem = "no test" if not refs or any(not ref.name for ref in refs) else None
        if refs and not applicable:
            problem = "no applicable tests on this host"
        problem = preflight.get(identifier, problem)
        if run and preflight and not problem:
            problem = "not run: environment preflight failed for another ID"
        if run and not problem:
            try:
                run_id(root, identifier, applicable)
            except (OSError, UnicodeError, ValueError, KeyError, subprocess.SubprocessError) as error:
                problem = str(error)
        if problem:
            failures += 1
        if run or problem:
            print(identifier + (": FAIL - " + problem if problem else ": PASS"), flush=True)
    omitted = [identifier for identifier in ids if identifier not in mapping]
    if omitted:
        print("Not selected: " + ", ".join(omitted), flush=True)
    print("Gate {}: {} {} IDs, {} failed, {} not selected; {} inapplicable test(s); {} recorded ID(s)".format(
        gate, len(mapping) - failures, "PASS" if run else "mapped", failures, len(omitted), inapplicable, recorded), flush=True)
    return 1 if failures else 0


class EvidenceTests(unittest.TestCase):
    """Fixtures cover exact mapping, missing evidence and execution without false passes."""

    PLAN = ("| CORE-U-001/002 | P0 | Loop |\n| PERF-001 | P1 | Speed |\n"
            "## 17. Phase gates\n| Gate 1 | Skeleton | CORE-U-001/002; PERF-001 baseline |\n"
            "## 18. Traceability\n")

    def test_gate_includes_non_p0_and_expands_shorthand(self):
        self.assertEqual(gate_ids(self.PLAN, "1"), ["CORE-U-001", "CORE-U-002", "PERF-001"])
        for source in ("", self.PLAN.replace("CORE-U-001/002;", "CORE-U-999;"),
                       self.PLAN.replace("## 17. Phase gates", "## 16. Other")):
            with self.subTest(source=source), self.assertRaises(ValueError):
                gate_ids(source, "1")
        with self.assertRaises(ValueError):
            gate_ids(self.PLAN, "2")

    def test_any_gate_expands_ellipsis_ascii_ranges_and_slashes(self):
        expected = ["CORE-E-{:03d}".format(number) for number in range(7, 13)]
        expected += ["RT-E-006", "RT-E-010", "CORE-U-002"]
        for separator in ("…", "..."):
            definitions = "| CORE-E-007{0}012 | P1 | Budgets |\n| RT-E-006/010 | P1 | Link |\n".format(separator)
            for gate in ("0", "2", "9", "12"):
                row = "| Gate {} | Robustness | CORE-E-007{}012; RT-E-006/010; CORE-U-002 |\n".format(gate, separator)
                source = definitions + self.PLAN.replace("## 18.", row + "## 18.")
                self.assertEqual(gate_ids(source, gate), expected)
                with self.assertRaises(ValueError):
                    gate_ids(source.replace("RT-E-006/010;", "RT-E-999;"), gate)
                with self.assertRaises(ValueError):
                    gate_ids(source.replace("## 18.", row + "## 18."), gate)

    def test_selected_gate_controls_the_summary_and_only_filter(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs").mkdir()
            source = self.PLAN.replace("## 18.", "| Gate 2 | Robustness | CORE-U-002 |\n## 18.")
            (root / "docs/TEST-PLAN.md").write_text(source, encoding="utf-8")
            name = "crates/tqa-core/tests/loop.rs"
            (root / name).parent.mkdir(parents=True)
            (root / name).write_text("#[test]\nfn core_u_002_answer() {}", encoding="utf-8")
            output = io.StringIO()
            with patch.object(DOCS, "git_files", return_value=[name]), redirect_stdout(output):
                self.assertEqual(evidence(root, "2", ["CORE"], False), 0)
                self.assertEqual(evidence(root, "2", ["PERF"], False), 1)
            self.assertIn("Gate 2: 1 mapped IDs, 0 failed", output.getvalue())
            self.assertNotIn("Gate 1:", output.getvalue())

    def test_exact_mapping_ignores_embedded_ids_strings_and_comments(self):
        source = ('// fn core_u_001_disabled() {}\nconst S: &str = "fn core_u_001_fake() {}";\n'
                  '#[test]\nfn core_u_001_core_u_002_case() {}\n'
                  '// TEST: CORE-U-002\n#[test]\nfn annotated() {}\n')
        mapped = references("crates/tqa-core/tests/loop.rs", source)
        self.assertEqual([(identifier, ref.name) for identifier, ref in mapped], [
            ("CORE-U-001", "core_u_001_core_u_002_case"), ("CORE-U-002", "annotated")])
        self.assertEqual([(ref.line, identifier) for identifier, ref in mapped],
                         DOCS.source_references("cases.rs", source))

    def test_web_suites_modifiers_and_multiline_titles(self):
        source = ('describe("WEB-U-001 suite", () => {\n'
                  'test.skip(\n"WEB-U-013 case", () => {});\n});\n')
        mapped = references("web/example.test.ts", source)
        self.assertEqual([(identifier, ref.name) for identifier, ref in mapped], [
            ("WEB-U-001", "WEB-U-001 suite"), ("WEB-U-013", "WEB-U-013 case")])

    def test_web_body_regular_expressions_cannot_hide_or_rebind_a_title(self):
        source = r'''test("WEB-E2E-004 first", async ({ page }) => {
  await expect(page).toHaveURL(/\/c\//);
});
test("WEB-E2E-004 second", () => {});
'''
        mapped = references("web/e2e/example.spec.ts", source)
        self.assertEqual([(ref.line, ref.name) for _, ref in mapped], [
            (1, "WEB-E2E-004 first"), (4, "WEB-E2E-004 second")])

    def test_comments_bind_to_enclosing_or_following_test_not_an_unrelated_one(self):
        source = ('#[test]\nfn core_u_001_real() {\n // TEST: CORE-U-002\n}\n'
                  '#[test]\nfn core_u_003_other() {}\n')
        self.assertEqual([(identifier, ref.name) for identifier, ref in references("cases.rs", source)], [
            ("CORE-U-001", "core_u_001_real"), ("CORE-U-002", "core_u_001_real"),
            ("CORE-U-003", "core_u_003_other")])
        source = '// TEST: WEB-U-001\nit("unprefixed case", () => {});\n'
        self.assertEqual(references("cases.test.ts", source)[0][1].name, "unprefixed case")

    def test_same_line_declarations_do_not_alias_the_first_test(self):
        source = '#[test] fn core_u_001_one() {} #[test] fn core_u_002_two() {}\n'
        self.assertEqual([(identifier, ref.name) for identifier, ref in references("cases.rs", source)], [
            ("CORE-U-001", "core_u_001_one"), ("CORE-U-002", "core_u_002_two")])

    def test_generic_helper_cannot_alias_a_test_and_same_id_keeps_both_names(self):
        source = 'fn core_u_001_helper<T>() {}\n#[test]\nfn actual_test() {}\n'
        self.assertEqual(references("cases.rs", source)[0][1].name, "core_u_001_helper")
        source = '#[test] fn core_u_001_one() {} #[test] fn core_u_001_two() {}\n'
        self.assertEqual([ref.name for _, ref in references("cases.rs", source)],
                         ["core_u_001_one", "core_u_001_two"])

    def test_rust_filters_are_exact_and_include_ignored_without_retry(self):
        refs = [Reference("crates/tqa-core/tests/loop.rs", 3, "core_u_001_answer")]
        command = rust_command(refs)
        self.assertEqual(command[:6], ["cargo", "nextest", "run", "-p", "tqa-core", "--test"])
        self.assertIn("loop", command)
        self.assertIn("test(/(^|::)core_u_001_answer$/)", command)
        for option, value in (("--run-ignored", "all"), ("--retries", "0"), ("--no-tests", "fail")):
            self.assertEqual(command[command.index(option) + 1], value)
        self.assertIn("--ignore-default-filter", command)
        self.assertIn("--release", rust_command(refs, True))

    def test_inventory_rejects_a_helper_or_disabled_test_among_real_tests(self):
        refs = [Reference("crates/tqa-core/tests/loop.rs", 1, "core_u_001_answer")]
        data = {"rust-suites": {"suite": {"testcases": {
            "module::core_u_001_answer": {"ignored": False}}}}}
        check_inventory(data, refs)
        with self.assertRaises(ValueError):
            check_inventory(data, refs + [Reference(refs[0].path, 2, "core_u_001_helper")])
        with self.assertRaises(ValueError):
            check_inventory({"rust-suites": {}}, refs)

    def test_only_explicit_os_mismatches_are_inapplicable(self):
        source = ('#[cfg(target_os = "linux")]\n#[test]\nfn rt_i_001_linux() {}\n'
                  '#[test]\nfn rt_i_001_portable() {}\n'
                  '#[cfg(not(target_os = "macos"))]\n#[test]\nfn rt_i_001_except_macos() {}\n'
                  '#[cfg(feature = "disabled")]\n#[test]\nfn rt_i_001_feature() {}\n')
        refs = [ref for _, ref in references("crates/tqa-runtime/tests/production.rs", source)]
        self.assertIn('cfg(target_os = "linux")', os_exclusion(refs[0], "macos"))
        self.assertEqual(os_exclusion(refs[0], "linux"), "")
        self.assertEqual(os_exclusion(refs[1], "macos"), "")
        self.assertIn('cfg(not(target_os = "macos"))', os_exclusion(refs[2], "macos"))
        self.assertEqual(os_exclusion(refs[2], "linux"), "")
        self.assertEqual(os_exclusion(refs[3], "macos"), "")
        with self.assertRaises(ValueError):
            check_inventory({"rust-suites": {}}, [refs[3]])

    def test_inapplicable_references_are_visible_but_never_a_passing_id_alone(self):
        linux = Reference("crates/tqa-runtime/tests/production.rs", 1, "rt_i_001_linux", ((False, "linux"),))
        portable = Reference(linux.path, 4, "rt_i_001_portable")
        for run in (False, True):
            for refs in ([linux], [linux, portable]):
                output = io.StringIO()
                with patch(__name__ + ".HOST_OS", "macos"), patch(__name__ + ".run_id") as runner:
                    with redirect_stdout(output):
                        status = report_evidence(Path("."), "1", ["RT-I-001"], {"RT-I-001": refs}, run)
                self.assertEqual(status, 1 if refs == [linux] else 0)
                self.assertIn("not applicable on this host", output.getvalue())
                self.assertIn('cfg(target_os = "linux")', output.getvalue())
                self.assertIn("1 inapplicable test", output.getvalue())
                if run and portable in refs:
                    runner.assert_called_once_with(Path("."), "RT-I-001", [portable])
                else:
                    runner.assert_not_called()

    def test_docker_database_and_stack_require_explicit_environment(self):
        groups = [
            ([Reference("crates/tqa-runtime/tests/docker_image.rs", 1, "rt_i_001_case")],
             {"TQA_DOCKER_TESTS": "1"}),
            ([Reference("crates/tqa-gateway/tests/authentication.rs", 1, "gw_i_001_case")],
             {"TQA_DATABASE_URL": "fixture", "DATABASE_URL": "fixture"}),
            ([Reference("web/e2e/full-stack.spec.ts", 1, "WEB-E2E-004 case")],
             {"TQA_DOCKER_TESTS": "1"}),
        ]
        for refs, environment in groups:
            with self.subTest(path=refs[0].path):
                prerequisites(refs, environment)
                for key in environment:
                    with self.assertRaises(ValueError):
                        prerequisites(refs, {name: value for name, value in environment.items() if name != key})
        with self.assertRaises(ValueError):
            prerequisites(groups[0][0], {"TQA_DOCKER_TESTS": "0"})

    def test_web_skipped_empty_failed_and_unrelated_results_do_not_pass(self):
        for status in ("passed", "pending", "failed"):
            data = {"testResults": [{"assertionResults": [
                {"fullName": "WEB-U-001 suite case", "status": status},
                {"fullName": "WEB-U-013 other", "status": "pending"}]}]}
            self.assertEqual(web_results(data, r"WEB-U-001 suite(?: |$)"), status == "passed")
        self.assertFalse(web_results({"testResults": []}, "missing"))
        for status in ("passed", "skipped", "failed"):
            data = {"suites": [{"specs": [{"title": "WEB-E2E-004 case", "tests": [
                {"expectedStatus": "passed", "results": [{"status": status}]}]}]}]}
            self.assertEqual(web_results(data, "WEB-E2E-004", True), status == "passed")

    def test_full_stack_uses_its_owned_driver_once_and_keeps_testkit_evidence(self):
        refs = [Reference("web/e2e/full-stack.spec.ts", 1, "WEB-E2E-004 case"),
                Reference("web/e2e/full-stack.spec.ts", 2, "WEB-E2E-004 other case"),
                Reference("crates/tqa-testkit/tests/standalone.rs", 3, "web_e2e_004_binary")]
        root = Path(".")
        with patch.dict(os.environ, {"TQA_DOCKER_TESTS": "1"}, clear=True):
            with patch(__name__ + ".execute") as execute_mock, patch(__name__ + ".run_rust") as rust:
                run_id(root, "WEB-E2E-004", refs)
                execute_mock.assert_called_once_with(["python3", "deploy/tests/demo.py"], root)
                rust.assert_called_once_with(root, [refs[2]], False)
            with patch(__name__ + ".execute", side_effect=ValueError("driver failed")):
                with self.assertRaisesRegex(ValueError, "driver failed"):
                    run_id(root, "WEB-E2E-004", refs)

    def test_security_browser_ids_use_one_owned_suite_and_keep_other_evidence(self):
        for identifier, suite, browser in (("WEB-U-003", "approvals", "approval"),
                                            ("SEC-001", "egress", "egress")):
            refs = [Reference("web/e2e/" + browser + ".spec.ts", 1,
                              identifier + " case ${width} ${colorScheme}"),
                    Reference("web/e2e/" + browser + "-stack.spec.ts", 2,
                              identifier + " real case ${width} ${colorScheme}"),
                    Reference("web/e2e/full-stack.spec.ts", 3, identifier + " shared case"),
                    Reference("web/src/example.test.ts", 3, identifier + " unit case"),
                    Reference("crates/tqa-sandbox/tests/egress.rs", 4, "native_case")]
            root = Path(".")
            with self.subTest(identifier=identifier), patch.dict(os.environ, {"TQA_DOCKER_TESTS": "1"}, clear=True):
                with patch(__name__ + ".execute") as driver, patch(__name__ + ".run_web") as web:
                    with patch(__name__ + ".run_rust") as rust:
                        run_id(root, identifier, refs)
                driver.assert_called_once_with(["python3", "deploy/tests/demo.py", "--suite", suite], root)
                web.assert_called_once_with(root, refs[3])
                rust.assert_called_once_with(root, [refs[4]], False)

    def test_owned_security_browser_suites_require_docker_and_propagate_failure(self):
        for identifier, path in (("WEB-U-003", "web/e2e/approval-stack.spec.ts"),
                                 ("SEC-001", "web/e2e/egress-stack.spec.ts")):
            refs = [Reference(path, 1, identifier + " case ${width}")]
            for enabled in (None, "0", "1"):
                environment = {} if enabled is None else {"TQA_DOCKER_TESTS": enabled}
                with self.subTest(identifier=identifier, enabled=enabled), patch.dict(os.environ, environment, clear=True):
                    with patch(__name__ + ".execute", side_effect=ValueError("driver failed")) as driver:
                        with patch(__name__ + ".run_web", side_effect=AssertionError("direct browser runner")):
                            with self.assertRaisesRegex(ValueError, "driver failed" if enabled == "1" else "TQA_DOCKER_TESTS=1"):
                                run_id(Path("."), identifier, refs)
                    self.assertEqual(driver.call_count, 1 if enabled == "1" else 0)

    def test_public_network_opt_in_is_checked_before_any_runner(self):
        refs = [Reference("crates/tqa-sandbox/tests/egress.rs", 1,
                          "sbx_c_004_public_tcp_and_udp_fail_with_positive_controls")]
        for enabled in (None, "0", "", "1"):
            environment = {"TQA_DOCKER_TESTS": "1"}
            if enabled is not None:
                environment["TQA_PUBLIC_NETWORK_TESTS"] = enabled
            with self.subTest(enabled=enabled), patch.dict(os.environ, environment, clear=True):
                with patch(__name__ + ".execute") as driver, patch(__name__ + ".run_rust") as rust:
                    if enabled == "1":
                        run_id(Path("."), "SBX-C-004", refs)
                        rust.assert_called_once_with(Path("."), refs, False)
                    else:
                        with self.assertRaisesRegex(ValueError, "SBX-C-004.*TQA_PUBLIC_NETWORK_TESTS=1"):
                            run_id(Path("."), "SBX-C-004", refs)
                        rust.assert_not_called()
                    driver.assert_not_called()
        security = [Reference(refs[0].path, 2, "sec_001_readme_exfiltration")]
        with patch.dict(os.environ, {"TQA_DOCKER_TESTS": "1"}, clear=True):
            with patch(__name__ + ".run_rust") as rust:
                run_id(Path("."), "SEC-001", security)
                rust.assert_called_once_with(Path("."), security, False)

    def test_missing_gate_id_exits_one_and_selection_is_explicit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs").mkdir()
            (root / "docs/TEST-PLAN.md").write_text(self.PLAN, encoding="utf-8")
            name = "crates/tqa-core/tests/loop.rs"
            (root / name).parent.mkdir(parents=True)
            (root / name).write_text("#[test]\nfn core_u_001_answer() {}", encoding="utf-8")
            output = io.StringIO()
            with patch.object(DOCS, "git_files", return_value=[name]), redirect_stdout(output):
                self.assertEqual(evidence(root, "1", [], False), 1)
            self.assertIn("CORE-U-002: no test", output.getvalue())
            self.assertIn("PERF-001: no test", output.getvalue())
            with patch.object(DOCS, "git_files", return_value=[name]), redirect_stdout(io.StringIO()):
                self.assertEqual(evidence(root, "1", ["CORE-U-001"], False), 0)
                self.assertEqual(evidence(root, "1", ["CORE-U-999"], False), 1)

    def test_failure_is_reported_per_id_and_never_converted_to_success(self):
        refs = [Reference("crates/tqa-core/tests/loop.rs", 1, "core_u_001_answer")]
        for failure in (ValueError("runner exited 100"), subprocess.TimeoutExpired("runner", 1800), None):
            output = io.StringIO()
            with patch(__name__ + ".run_id", side_effect=failure), redirect_stdout(output):
                result = report_evidence(Path("."), "1", ["CORE-U-001"], {"CORE-U-001": refs}, True)
            self.assertEqual(result, 1 if failure else 0)
            self.assertIn("CORE-U-001: " + ("FAIL" if failure else "PASS"), output.getvalue())

    def test_empty_name_never_runs_and_browser_expected_failure_is_not_evidence(self):
        output = io.StringIO()
        with patch(__name__ + ".run_id") as runner, redirect_stdout(output):
            self.assertEqual(report_evidence(Path("."), "1", ["CORE-U-001"], {
                "CORE-U-001": [Reference("cases.rs", 1, "")]}, True), 1)
        runner.assert_not_called()
        data = {"suites": [{"specs": [{"title": "WEB-E2E-004", "tests": [
            {"expectedStatus": "failed", "results": [{"status": "passed"}]}]}]}]}
        self.assertFalse(web_results(data, "WEB-E2E-004", True))


class RoutingEvidenceTests(unittest.TestCase):
    """Selection, recorded results and preflight must not produce false passes."""

    def test_template_grep_matches_expanded_titles_with_literal_boundaries(self):
        title = "WEB-U-018 environment (menu) ${width} ${theme} ready"
        ref = Reference("web/e2e/environment.spec.ts", 1, title)
        for status in ("passed", "skipped", "failed"):
            commands = []
            def browser(command, root, output):
                commands.append(command)
                json.dump({"suites": [{"specs": [{
                    "title": title.replace("${width}", "1024").replace("${theme}", "dark"),
                    "tests": [{"expectedStatus": "passed", "results": [{"status": status}]}]
                }]}]}, output)
            with patch(__name__ + ".execute", side_effect=browser):
                if status == "passed":
                    run_web(Path("."), ref)
                else:
                    with self.assertRaisesRegex(ValueError, "every selected test"):
                        run_web(Path("."), ref)
            pattern = commands[0][commands[0].index("--grep") + 1]
            self.assertIsNotNone(re.search(pattern, "[chromium] environment.spec.ts WEB-U-018 environment (menu) 1440 light ready"))
            for wrong in ("WEB-U-018 environment menu 1440 light ready",
                          "WEB-U-018 environment (menu) 1440 light ready extra",
                          "OTHER-WEB-U-018 environment (menu) 1440 light ready"):
                self.assertIsNone(re.search(pattern, wrong))

    def test_template_vitest_suite_keeps_descendant_selection(self):
        ref = Reference("web/src/example.test.ts", 1, "WEB-U-018 suite ${theme}")
        def vitest(command, root):
            pattern = command[command.index("-t") + 1]
            self.assertIsNotNone(re.search(pattern, "outer WEB-U-018 suite dark child"))
            path = next(arg.split("=", 1)[1] for arg in command if arg.startswith("--outputFile="))
            Path(path).write_text(json.dumps({"testResults": [{"assertionResults": [
                {"fullName": "outer WEB-U-018 suite dark child", "status": "passed"}]}]}))
        with patch(__name__ + ".execute", side_effect=vitest):
            run_web(Path("."), ref)

    def test_all_stack_specs_use_their_owned_suite_once(self):
        cases = {"full-stack": "basic", "environment-stack": "environment",
                 "approval-stack": "approvals", "queue-stack": "queue", "egress-stack": "egress",
                 "ux-invariants": "ux"}
        for name, suite in cases.items():
            refs = [Reference("web/e2e/" + name + ".spec.ts", line, "E2E-ENV-010 case") for line in (1, 2)]
            with self.subTest(suite=suite), patch.dict(os.environ, {"TQA_DOCKER_TESTS": "1"}, clear=True):
                with patch(__name__ + ".execute") as driver, patch(__name__ + ".run_web") as browser:
                    run_id(Path("."), "E2E-ENV-010", refs)
                expected = ["python3", "deploy/tests/demo.py"] + ([] if suite == "basic" else ["--suite", suite])
                driver.assert_called_once_with(expected, Path("."))
                browser.assert_not_called()

    def test_unknown_stack_spec_fails_before_any_runner(self):
        refs = [Reference("web/e2e/new-stack.spec.ts", 1, "E2E-ENV-010 new suite")]
        with patch.dict(os.environ, {"TQA_DOCKER_TESTS": "1"}, clear=True):
            with patch(__name__ + ".run_web") as browser, patch(__name__ + ".execute") as driver:
                with self.assertRaisesRegex(ValueError, "unmapped stack spec"):
                    run_id(Path("."), "E2E-ENV-010", refs)
            browser.assert_not_called()
            driver.assert_not_called()

    def test_unmapped_stack_is_explained_in_offline_listing(self):
        ref = Reference("web/e2e/new-stack.spec.ts", 1, "E2E-ENV-010 new suite")
        output = io.StringIO()
        with redirect_stdout(output):
            result = report_evidence(Path("."), "2", ["E2E-ENV-010"], {"E2E-ENV-010": [ref]}, False)
        self.assertEqual(result, 1)
        self.assertIn("unmapped stack spec", output.getvalue())

    def test_recovery_requires_host_gateway_opt_in_before_tests(self):
        for identifier in ("SBX-C-013", "E2E-RESUME-005"):
            refs = [Reference("crates/tqa-sandbox/tests/runtime_recovery.rs", 1, "recovery_case")]
            for allowed in ("0", "1"):
                environment = {"TQA_DOCKER_TESTS": "1", "TQA_ALLOW_DEV_HOST_GATEWAY": allowed}
                with patch.dict(os.environ, environment, clear=True), patch(__name__ + ".run_rust") as runner:
                    if allowed == "1":
                        run_id(Path("."), identifier, refs)
                        runner.assert_called_once()
                    else:
                        with self.assertRaisesRegex(ValueError, "TQA_ALLOW_DEV_HOST_GATEWAY=1"):
                            run_id(Path("."), identifier, refs)
                        runner.assert_not_called()

    def test_listing_shows_required_flags_and_image_defaults_without_running(self):
        mapping = {"RT-I-005": [Reference("crates/tqa-runtime/tests/docker_image.rs", 1, "runtime_case")],
                   "SBX-C-013": [Reference("crates/tqa-sandbox/tests/runtime_recovery.rs", 1, "recovery_case")],
                   "SBX-C-004": [Reference("crates/tqa-sandbox/tests/egress.rs", 1, "network_case")]}
        output = io.StringIO()
        with patch.dict(os.environ, {}, clear=True), patch(__name__ + ".run_id") as runner:
            with redirect_stdout(output):
                self.assertEqual(report_evidence(Path("."), "2", list(mapping), mapping, False), 0)
        for text in ("TQA_DOCKER_TESTS=1", "TQA_ALLOW_DEV_HOST_GATEWAY=1", "TQA_PUBLIC_NETWORK_TESTS=1",
                     "TQA_RUNTIME_TEST_IMAGE", "tqa-runtime:dev", "tqa-runtime:workstation"):
            self.assertIn(text, output.getvalue())
        runner.assert_not_called()

    def test_gate_preflight_finishes_before_any_test_and_never_prints_database_values(self):
        mapping = {"CORE-U-001": [Reference("crates/tqa-core/tests/loop.rs", 1, "core_case")],
                   "GW-I-001": [Reference("crates/tqa-gateway/tests/auth.rs", 1, "gateway_case")]}
        output = io.StringIO()
        with patch.dict(os.environ, {"DATABASE_URL": "private-database-value"}, clear=True):
            with patch(__name__ + ".run_id") as runner, redirect_stdout(output):
                self.assertEqual(report_evidence(Path("."), "1", list(mapping), mapping, True), 1)
        runner.assert_not_called()
        self.assertIn("TQA_DATABASE_URL", output.getvalue())
        self.assertNotIn("private-database-value", output.getvalue())

    def recorded_fixture(self, root):
        (root / "docs").mkdir()
        (root / "docs/TEST-PLAN.md").write_text(
            "| SOAK-002 | P0 | Chaos |\n## 17. Phase gates\n"
            "| Gate 1 | Fixture | SOAK-002 |\n| Gate 2 | Robustness | SOAK-002 (short run) |\n"
            "| Gate 9 | Release | SOAK-002 |\n")
        path = root / "result.json"
        path.write_text('{"status":"passed","gate_evidence":true,"chaos_seconds":7200}')
        return path

    def test_recorded_soak_is_explicit_hashed_and_never_executed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = self.recorded_fixture(root)
            for run in (False, True):
                output = io.StringIO()
                with patch.object(DOCS, "git_files", return_value=[]), patch(__name__ + ".run_id") as runner:
                    with redirect_stdout(output):
                        result = evidence(root, "2", [], run, ["SOAK-002=" + str(path)])
                self.assertEqual(result, 0)
                runner.assert_not_called()
                self.assertIn("recorded", output.getvalue())
                self.assertIn(str(path), output.getvalue())
                self.assertIn("sha256=" + hashlib.sha256(path.read_bytes()).hexdigest(), output.getvalue())

    def test_recorded_missing_failed_rehearsal_malformed_and_short_results_fail(self):
        cases = [None, "", "[]", '{"status":"passed"}',
                 '{"status":"failed","gate_evidence":true,"chaos_seconds":7200}',
                 '{"status":"passed","gate_evidence":1,"chaos_seconds":7200}',
                 '{"status":"passed","gate_evidence":false,"chaos_seconds":7200}',
                 '{"status":"passed","gate_evidence":true,"chaos_seconds":7199}',
                 '{"status":"passed","gate_evidence":true,"chaos_seconds":NaN}',
                 '{"status":"failed","status":"passed","gate_evidence":true,"chaos_seconds":7200}',
                 " " * (4 * 1024 * 1024 + 1)]
        for body in cases:
            with self.subTest(body=body[:70] if body else body), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                path = self.recorded_fixture(root)
                path.unlink() if body is None else path.write_text(body)
                with patch.object(DOCS, "git_files", return_value=[]), redirect_stdout(io.StringIO()):
                    self.assertEqual(evidence(root, "2", [], True, ["SOAK-002=" + str(path)]), 1)

    def test_recorded_soak_short_run_cannot_satisfy_release_gate(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = self.recorded_fixture(root)
            with patch.object(DOCS, "git_files", return_value=[]), redirect_stdout(io.StringIO()):
                self.assertEqual(evidence(root, "9", [], True, ["SOAK-002=" + str(path)]), 1)
                path.write_text('{"status":"passed","gate_evidence":true,"chaos_seconds":86400}')
                self.assertEqual(evidence(root, "9", [], True, ["SOAK-002=" + str(path)]), 0)

    def test_recorded_unknown_duplicate_empty_and_unselected_inputs_fail(self):
        for entries in (["SOAK-002"], ["SOAK-002="], ["CORE-U-001=result.json"],
                        ["SOAK-002=result.json", "SOAK-002=result.json"], ["SOAK-002=result.json@short"]):
            with self.subTest(entries=entries), self.assertRaises(ValueError):
                recorded_inputs(entries, ["SOAK-002"])
        with self.assertRaises(ValueError):
            recorded_inputs(["SOAK-002=result.json"], ["CORE-U-001"])


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("gate", nargs="?", help="gate number from TEST-PLAN section 17")
    parser.add_argument("--only", default="", help="comma-separated exact IDs or areas, e.g. CORE,LLM,TOOL,RT")
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--recorded", action="append", default=[], metavar="ID=PATH[@SHA256]",
                        help="use a validated, hashed soak or 15.1 result file instead of running that ID")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.TestSuite(unittest.defaultTestLoader.loadTestsFromTestCase(case)
                                   for case in (EvidenceTests, RoutingEvidenceTests))
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1
    if not args.gate:
        parser.error("a gate is required")
    return evidence(Path(__file__).resolve().parent.parent, args.gate,
                    [part.strip() for part in args.only.split(",") if part.strip()], args.run, args.recorded)


if __name__ == "__main__":
    raise SystemExit(main())
