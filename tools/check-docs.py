#!/usr/bin/env python3
"""G-09: check local Markdown links and exact TEST-PLAN references without network access."""

import argparse
from contextlib import redirect_stdout
import io
from pathlib import Path
import re
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from docs_markdown import blank, check_markdown, heading_slugs, markdown_links, paired_end


ID_LIST = re.compile(r"\b([A-Z][A-Z0-9]*(?:-[A-Z][A-Z0-9]*)*)-(\d{3,})((?:(?:/|…|\.\.\.)\d{3,})*)(?![\w-])")
SOURCE_SUFFIXES = {".rs", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"}
MAX_ID_RANGE = 10000


def expand_ids(text):
    """Expand exact IDs and TEST-PLAN's slash/range shorthand, preserving each prefix."""
    identifiers = []
    for match in ID_LIST.finditer(text):
        prefix, first, continuation = match.groups()
        previous = int(first)
        identifiers.append(prefix + "-" + first)
        for separator, number in re.findall(r"(/|…|\.\.\.)(\d+)", continuation):
            last = int(number)
            if separator != "/" and not 0 <= last - previous <= MAX_ID_RANGE:
                raise ValueError("invalid or excessive test-ID range")
            values = [last] if separator == "/" else range(previous + 1, last + 1)
            identifiers.extend("{}-{:0{}d}".format(prefix, value, len(first)) for value in values)
            previous = last
    return identifiers


def test_plan(source):
    """Read only definition rows; return their locations and the Gate 1 P0 subset."""
    rows, required = {}, None
    for line, text in enumerate(source.splitlines(), 1):
        cells = [cell.strip() for cell in text.split("|")]
        if len(cells) < 5:
            continue
        if cells[1] == "Gate 1":
            required = set(expand_ids(cells[3]))
        elif cells[2] in {"P0", "P1", "P2", "–"}:
            for identifier in expand_ids(cells[1]):
                if identifier in rows:
                    raise ValueError("duplicate test-plan row: " + identifier)
                rows[identifier] = (line, cells[2], cells[3])
    if not rows or required is None:
        raise ValueError("test plan must contain definition rows and the Gate 1 row")
    if required - rows.keys():
        raise ValueError("Gate 1 names undefined IDs: " + ", ".join(sorted(required - rows.keys())))
    return rows, {identifier for identifier in required if rows[identifier][1] == "P0"}


def source_tokens(source):
    """Yield comments and literals so quoted examples cannot invent test references."""
    pattern = re.compile(r'//[^\n]*|/\*|(?:b?r)(?P<hashes>\#*)".*?"(?P=hashes)|'
                         r'"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\\n])*\'|`(?:\\.|[^`\\])*`', re.S)
    position = 0
    while True:
        match = pattern.search(source, position)
        if not match:
            return
        start, end = match.span()
        if match[0] == "/*":
            depth = 1
            for delimiter in re.finditer(r"/\*|\*/", source[end:]):
                depth += 1 if delimiter[0] == "/*" else -1
                if depth == 0:
                    end += delimiter.end()
                    break
            else:
                end = len(source)
        yield start, end, source[start:end]
        position = end


def web_title_ids(source, code, literals):
    """Read titles from test/suite calls, including modifier chains and table-driven tests."""
    for match in re.finditer(r"\b(?:test|it|describe)(?:\.\w+)*\s*\(", code):
        position = match.end()
        if ".each" in match[0]:
            close = paired_end(code, position - 1, "(", ")")
            if close is None:
                continue
            next_call = re.match(r"\s*\(\s*", source[close + 1:])
            if not next_call:
                continue
            position = close + 1 + next_call.end()
        else:
            position += len(source[position:]) - len(source[position:].lstrip())
        if position in literals:
            for identifier in expand_ids(literals[position]):
                yield position, identifier


def source_references(name, source):
    """Map Rust function prefixes, web titles and TEST comments to exact plan IDs."""
    code, references, literals = list(source), [], {}
    for start, end, token in source_tokens(source):
        code[start:end] = blank(token)
        if token.startswith("//"):
            directive = re.match(r"//\s*TEST:\s*(.*)", token)
            if directive:
                references.extend((start, identifier) for identifier in expand_ids(directive[1]))
        elif not token.startswith("/*"):
            literals[start] = token
    code = "".join(code)
    if Path(name).suffix == ".rs":
        for match in re.finditer(r"\bfn\s+([a-z][a-z0-9_]*)", code):
            identifier = re.match(r"([a-z][a-z0-9]*(?:_[a-z][a-z0-9]*)*_\d{3,})(?=_|$)", match[1])
            if identifier:
                references.append((match.start(), identifier[1].upper().replace("_", "-")))
    else:
        references.extend(web_title_ids(source, code, literals))
    return [(source.count("\n", 0, offset) + 1, identifier)
            for offset, identifier in sorted(references, key=lambda item: item[0])]


def git_files(root):
    """Select tracked files, even when ignored, plus new files Git does not ignore."""
    command = ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"]
    result = subprocess.run(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            encoding="utf-8", check=True, timeout=30)
    return sorted(set(filter(None, result.stdout.split("\0"))))


def run_check(root, list_external=False):
    """Report every violation and the nonblocking Gate 1 coverage list, then summarize."""
    problems, referenced, external = [], set(), []
    try:
        rows, required = test_plan((root / "docs/TEST-PLAN.md").read_text(encoding="utf-8"))
        names = git_files(root)
        documents = [name for name in names if Path(name).suffix.lower() in {".md", ".markdown"}]
        sources = [name for name in names if Path(name).suffix in SOURCE_SUFFIXES]
        for name in documents + sources:
            try:
                source = (root / name).read_text(encoding="utf-8")
            except (OSError, UnicodeError):
                problems.append((name, 1, "cannot read source file"))
                continue
            if name in documents:
                problems.extend((name, line, message) for line, message in check_markdown(root, name, source))
                external.extend((name, line, target) for line, target in markdown_links(source)
                                if target.startswith(("http://", "https://")))
            else:
                for line, identifier in source_references(name, source):
                    if identifier not in rows:
                        problems.append((name, line, identifier + " is not defined in docs/TEST-PLAN.md"))
                    else:
                        referenced.add(identifier)
    except (OSError, UnicodeError, ValueError, subprocess.SubprocessError) as error:
        print("docs/TEST-PLAN.md:1: G-09 cannot read repository inputs: " + str(error))
        print("G-09: 1 violation")
        return 1
    for name, line, message in sorted(problems):
        print("{}:{}: {}".format(name, line, message))
    if list_external:
        for name, line, target in sorted(set(external)):
            print("{}:{}: external (not fetched): {}".format(name, line, target))
    for identifier in sorted(required - referenced):
        line, _, title = rows[identifier]
        print("docs/TEST-PLAN.md:{}: report-only: Gate 1 P0 {} unreferenced ({})".format(line, identifier, title))
    print("G-09: Gate 1 P0 report-only: {} of {} IDs unreferenced (references do not prove acceptance)".format(
        len(required - referenced), len(required)))
    print("G-09: {} violations; {} Markdown files; {} source files; {} defined IDs referenced".format(
        len(problems), len(documents), len(sources), len(referenced)))
    return 1 if problems else 0


class DocumentationTests(unittest.TestCase):
    """Embedded fixtures cover rendered links, heading collisions and test traceability."""

    def test_empty_markdown(self):
        self.assertEqual(markdown_links(""), [])
        self.assertEqual(heading_slugs(""), set())

    def test_heading_markup_unicode_and_collisions(self):
        source = """# Same
# Same
# Same-1
# Same
## Hello, *World*! `code_name` &amp; café 中文 🚀
## [Linked heading](guide.md) ###
Setext title
============
"""
        self.assertEqual(heading_slugs(source), {
            "same", "same-1", "same-1-1", "same-2",
            "hello-world-code_name--café-中文-", "linked-heading", "setext-title",
        })

    def test_heading_code_and_escaped_markup_stay_literal(self):
        self.assertEqual(heading_slugs("## `<tag>` &amp; \\_literal\\_ `&amp;`\n"), {
            "tag--_literal_-amp",
        })

    def test_examples_and_comments_are_not_links_or_headings(self):
        source = """```md
# Hidden
[bad](absent.md)
```
~~~
# Also hidden
~~~
    [indented](absent.md)
<!-- [hidden](absent.md)
# Hidden too
-->
`[inline](absent.md)` and ``[inline](absent.md)``
\\[escaped](absent.md)
[visible](ok.md)
"""
        self.assertEqual(markdown_links(source), [(14, "ok.md")])
        self.assertEqual(heading_slugs(source), set())

    def test_inline_images_titles_and_balanced_paths(self):
        source = ('[one](guide(1).md#topic "a title") ![image](image.svg)\n'
                  '[two](<space file.md#part>) [three](escaped\\(path\\).md)\n'
                  '[a `code` label](ok.md) [nested [label]](ok.md)\n'
                  '[multiple\nlines](ok.md)')
        self.assertEqual(markdown_links(source), [
            (1, "guide(1).md#topic"), (1, "image.svg"),
            (2, "space file.md#part"), (2, "escaped(path).md"),
            (3, "ok.md"), (3, "ok.md"), (4, "ok.md"),
        ])

    def test_image_nested_in_a_link_checks_both_destinations(self):
        self.assertEqual(markdown_links("[![badge](badge.svg)](page.md)"), [
            (1, "page.md"), (1, "badge.svg"),
        ])

    def test_reference_full_collapsed_shortcut_and_external_links(self):
        source = ('[one][Guide] [guide][] [guide]\n'
                  '[Guide]: guide.md#topic "title"\n'
                  '[site](https://example.org/docs) <http://example.org/>\n'
                  '<a href="guide.md#topic">HTML</a>\n')
        self.assertEqual(markdown_links(source), [
            (1, "guide.md#topic"), (1, "guide.md#topic"), (1, "guide.md#topic"),
            (2, "guide.md#topic"), (3, "https://example.org/docs"),
            (3, "http://example.org/"), (4, "guide.md#topic"),
        ])

    def test_external_autolinks_do_not_include_surrounding_prose_or_emphasis(self):
        self.assertEqual(markdown_links("**https://example.org/docs** (https://example.org/a_(b))."), [
            (1, "https://example.org/docs"), (1, "https://example.org/a_(b)"),
        ])

    def test_footnote_prose_is_not_a_file_link(self):
        self.assertEqual(markdown_links("See [^note].\n[^note]: Read [guide](guide.md).\n"), [
            (2, "guide.md"),
        ])

    def test_relative_links_and_anchors(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs").mkdir()
            (root / "docs/space file.md").write_text("# Café\n# Café\n", encoding="utf-8")
            source = ('[ok](docs/space%20file.md#caf%C3%A9-1)\n'
                      '[missing](absent.md)\n[wrong](docs/space%20file.md#nope)\n'
                      '[directory](docs)\n[external](https://example.org/missing#nope)\n')
            problems = check_markdown(root, "README.md", source)
            self.assertEqual([line for line, _ in problems], [2, 3, 4])
            self.assertIn("absent.md", problems[0][1])
            self.assertIn("nope", problems[1][1])

    def test_self_anchor_and_missing_symlink_target(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "broken.md").symlink_to("missing.md")
            source = "# Topic\n[ok](#topic)\n[bad](#missing)\n[bad](broken.md)\n"
            self.assertEqual([line for line, _ in check_markdown(root, "README.md", source)], [3, 4])

    def test_symlink_cannot_validate_a_link_outside_the_repository(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "repo"
            root.mkdir()
            (root.parent / "outside.md").write_text("# Topic\n", encoding="utf-8")
            (root / "escape.md").symlink_to("../outside.md")
            self.assertEqual(len(check_markdown(root, "README.md", "[bad](escape.md#topic)")), 1)

    def test_exact_rust_ids_include_unknown_areas_and_numbers(self):
        source = """#[test]
fn tool_u_001_reads() {}
#[tokio::test]
async fn perf_050_speed() {}
fn e2e_resume_005_restart() {}
fn unknown_u_999_missing() {}
fn tool_u_1001_missing() {}
"""
        self.assertEqual(source_references("tests/cases.rs", source), [
            (2, "TOOL-U-001"), (4, "PERF-050"), (5, "E2E-RESUME-005"),
            (6, "UNKNOWN-U-999"), (7, "TOOL-U-1001"),
        ])

    def test_source_strings_and_disabled_code_do_not_count(self):
        source = '''// fn unknown_u_998_fake() {}
/* fn unknown_u_997_fake() {} */
const EXAMPLE: &str = r##"fn unknown_u_996_fake() {}"##;
const TEXT: &str = "// TEST: UNKNOWN-U-995";
// TEST: TOOL-U-001, PERF-050
fn normal_helper() {}
'''
        self.assertEqual(source_references("src/test.rs", source), [(5, "TOOL-U-001"), (5, "PERF-050")])

    def test_web_titles_include_suites_modifiers_templates_and_each(self):
        source = '''describe("WEB-U-001 suite", () => {});
test.skip('WEB-E2E-004 case', () => {});
it.each([1, 2])(`WEB-U-007 ${theme}`, () => {});
test("UNKNOWN-U-999 fails", () => {});
// test("UNKNOWN-U-998 disabled", () => {});
const sample = "UNKNOWN-U-997 example";
// TEST: GW-I-016
'''
        self.assertEqual(source_references("web/cases.test.ts", source), [
            (1, "WEB-U-001"), (2, "WEB-E2E-004"), (3, "WEB-U-007"),
            (4, "UNKNOWN-U-999"), (7, "GW-I-016"),
        ])

    def test_plan_rows_are_authoritative_not_incidental_mentions(self):
        source = """Mention TOOL-U-999 without defining it.
| TOOL-U-001 | P0 | Read |
| TOOL-U-002 | P1 | Write |
| TOOL-U-003 | P0 | Shell |
| PERF-050 | P1 | Speed |
| Gate 1 | Skeleton | TOOL-U-001/002/003; PERF-050 |
"""
        rows, gate_one = test_plan(source)
        self.assertEqual(set(rows), {"TOOL-U-001", "TOOL-U-002", "TOOL-U-003", "PERF-050"})
        self.assertEqual(gate_one, {"TOOL-U-001", "TOOL-U-003"})

    def test_missing_gate_or_no_plan_rows_fails_closed(self):
        for source in ("", "| TOOL-U-001 | P0 | Read |"):
            with self.subTest(source=source), self.assertRaises(ValueError):
                test_plan(source)

    def test_id_lists_expand_ranges_without_losing_type(self):
        self.assertEqual(expand_ids("TOOL-S-001/003, PERF-050…052; E2E-RESUME-005"), [
            "TOOL-S-001", "TOOL-S-003", "PERF-050", "PERF-051", "PERF-052", "E2E-RESUME-005",
        ])

    def test_gate_rejects_unknown_ids_and_reports_unreferenced_p0_without_failing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs").mkdir()
            (root / "docs/TEST-PLAN.md").write_text(
                "| TOOL-U-001 | P0 | Read |\n| Gate 1 | Skeleton | TOOL-U-001 |\n", encoding="utf-8")
            fixture = root / "test.rs"
            fixture.write_text("#[test]\nfn unknown_u_999_wrong() {}\n", encoding="utf-8")
            for invalid in (True, False):
                with self.subTest(invalid=invalid):
                    if not invalid:
                        fixture.write_text("fn helper() {}\n", encoding="utf-8")
                    output = io.StringIO()
                    with patch(__name__ + ".git_files", return_value=["docs/TEST-PLAN.md", "test.rs"]):
                        with redirect_stdout(output):
                            status = run_check(root)
                    self.assertEqual(status, 1 if invalid else 0)
                    self.assertIn("report-only: Gate 1 P0 TOOL-U-001 unreferenced", output.getvalue())
                    if invalid:
                        self.assertIn("test.rs:2: UNKNOWN-U-999 is not defined", output.getvalue())

    def test_real_git_checks_new_markdown_without_staging_and_excludes_ignored_files(self):
        repository = Path(__file__).resolve().parent.parent
        with tempfile.TemporaryDirectory(dir=repository / "tools", prefix="docs-fixture-") as directory:
            root = Path(directory)
            (root / "docs").mkdir()
            (root / "docs/TEST-PLAN.md").write_text(
                "| TOOL-U-001 | P0 | Read |\n| Gate 1 | Skeleton | TOOL-U-001 |\n", encoding="utf-8")
            (root / ".gitignore").write_text("ignored.md\n", encoding="utf-8")
            (root / "ignored.md").write_text("[bad](absent.md)\n", encoding="utf-8")
            for name in ("new guide.md", "new guide.markdown"):
                with self.subTest(name=name):
                    fixture = root / name
                    fixture.write_text("[bad](absent.md)\n", encoding="utf-8")
                    output = io.StringIO()
                    with redirect_stdout(output):
                        status = run_check(root)
                    self.assertEqual(status, 1, output.getvalue())
                    self.assertIn(name + ":1: link is not an existing file: absent.md", output.getvalue())
                    self.assertNotIn("ignored.md", output.getvalue())
                    fixture.write_text("# Topic\n[good](#topic)\n", encoding="utf-8")
                    output = io.StringIO()
                    with redirect_stdout(output):
                        status = run_check(root)
                    self.assertEqual(status, 0, output.getvalue())
                    self.assertIn("0 violations; 2 Markdown files", output.getvalue())
                    fixture.unlink()


def main():
    """Run the gate or its offline embedded fixtures."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--list-external", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(DocumentationTests)
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1
    return run_check(Path(__file__).resolve().parent.parent, args.list_external)


if __name__ == "__main__":
    raise SystemExit(main())
