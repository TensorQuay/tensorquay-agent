"""G-09-complete: every document the plans name exists. Each rule on a small fixture tree: what passes, what fails and
where the failure is reported."""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parent / "check-docs-complete.py"
SPEC = importlib.util.spec_from_file_location("check_docs_complete", SOURCE)
complete = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(complete)

DOCUMENTS = {"admin guide": "docs/admin-guide/README.md", "protocol reference": "docs/protocol-reference.md"}


SCHEMA = {"definitions": {
    "ClientFrame": {"oneOf": [{"properties": {"type": {"const": "user.message"}}}]},
    "ServerFrame": {"oneOf": [{"properties": {"type": {"const": "event"}}}]},
    "Event": {"oneOf": [{"properties": {"type": {"const": "run.started"}}}]},
    "ErrorCode": {"oneOf": [{"const": "seq_gap"}, {"enum": ["busy"]}]},
    "RunId": {"type": "string"},
}}
REFERENCE = """# Protocol reference

| Type | Meaning |
|---|---|
| `user.message` | send |
| `event` | an event |
| `run.started` | a run begins |
| `seq_gap` | reconnect |
| `busy` | later |

Other types: `ClientFrame`, `ServerFrame`, `Event`, `ErrorCode`, `RunId`.
"""


class Tree:
    """A temporary repository with a plan, a crate, the copy and a guide that pass every rule."""

    def __init__(self, directory):
        self.root = Path(directory)
        self.write("docs/DEV-PLAN.md", "Decided in ADR-0001. The testkit has `FakeClock`. See the admin guide.\n")
        self.write("docs/adr/ADR-0001-loop.md", "# ADR-0001: the loop\n")
        self.write("crates/tqa-testkit/src/lib.rs", "pub struct FakeClock;\n")
        self.write("docs/admin-guide/README.md", "# Admin guide\n\n## Environments\n")
        self.write(complete.SCHEMA, json.dumps(SCHEMA))
        self.write(complete.PROTOCOL_REFERENCE, REFERENCE)
        self.copy({"environments": {"noImages": "Build them; the admin guide's Environments section says how."}})

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def copy(self, catalogue):
        self.write(complete.COPY, json.dumps(catalogue))

    def problems(self, documents=DOCUMENTS):
        return (complete.check_adrs(self.root) + complete.check_fakes(self.root)
                + complete.check_named_documents(self.root, documents) + complete.check_guide_sections(self.root)
                + complete.check_protocol_reference(self.root))


class DocsCompleteTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.tree = Tree(directory.name)

    def test_a_complete_tree_passes(self):
        self.assertEqual(self.tree.problems(), [])
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(complete.run_check(self.tree.root, DOCUMENTS), 0)
        self.assertIn("G-09-complete: 0 missing document(s)", output.getvalue())

    def test_a_cited_adr_without_its_record_fails_at_its_first_citation(self):
        self.tree.write("docs/ARCHITECTURE.md", "Line one.\nEgress: ADR-0004.\nAgain ADR-0004 and ADR-0001.\n")
        self.assertEqual(self.tree.problems(), [
            ("docs/ARCHITECTURE.md:2", "ADR-0004 is cited (2 times) but docs/adr/ADR-0004-*.md does not exist")])

    def test_an_adr_cited_only_by_another_adr_counts(self):
        self.tree.write("docs/adr/ADR-0001-loop.md", "# ADR-0001\n\nBinds a push (ADR-0008).\n")
        [(place, message)] = self.tree.problems()
        self.assertEqual(place, "docs/adr/ADR-0001-loop.md:3")
        self.assertIn("ADR-0008 is cited (1 times)", message)

    def test_an_adr_file_needs_its_title_after_the_number(self):
        self.tree.write("docs/adr/ADR-0002.md", "# no title in the name\n")
        self.tree.write("docs/PRD.md", "Rust (ADR-0002).\n")
        self.assertEqual([place for place, _ in self.tree.problems()], ["docs/PRD.md:1"])

    def test_a_named_fake_must_be_a_public_type_or_re_export(self):
        self.tree.write("docs/DEV-GUIDELINES.md", "Fakes: `FakeSandbox`, `FakeRelay`, `FakeProvider`, `FakeServer`.\n")
        self.tree.write("crates/tqa-testkit/src/sandbox.rs", "pub(crate) struct FakeSandbox;\n// FakeRelay is planned\n")
        self.tree.write("crates/tqa-testkit/src/lib.rs",
                        "pub struct FakeClock;\npub use provider::{\n    FakeProvider,\n    Other as FakeServer,\n};\n")
        self.assertEqual(self.tree.problems(), [
            ("docs/DEV-GUIDELINES.md:1", "`FakeRelay` is named as a testkit fake but no crate defines or re-exports it"),
            ("docs/DEV-GUIDELINES.md:1", "`FakeSandbox` is named as a testkit fake but no crate defines or re-exports it"),
        ])

    def test_a_fake_without_backticks_is_prose_and_not_checked(self):
        self.tree.write("docs/TEST-PLAN.md", "A FakeWidget in prose is not a name the plan gives.\n")
        self.assertEqual(self.tree.problems(), [])

    def test_a_named_document_needs_an_entry_and_its_file(self):
        self.tree.write("docs/PRD.md", "Install guide, admin guide, protocol reference.\nThe Protocol Reference again.\n")
        documents = dict(DOCUMENTS, **{"protocol reference": None})
        self.assertEqual(self.tree.problems(documents), [
            ("docs/PRD.md:1", "the install guide is named but NAMED_DOCUMENTS has no entry for it"),
            ("docs/PRD.md:1", "the protocol reference is named but no file is it yet"),
        ])
        (self.tree.root / "docs/admin-guide/README.md").unlink()
        self.tree.write("docs/admin-guide/index.md", "## Environments\n")
        self.assertIn(("docs/PRD.md:1", "the admin guide is named but docs/admin-guide/README.md does not exist"),
                      self.tree.problems())

    def test_a_guide_section_the_copy_names_must_be_a_heading(self):
        self.tree.copy({"a": {"b": "See the admin guide's Restarting the gateway section.",
                              "c": "Or the user guide's Tasks section."}})
        self.tree.write("docs/user-guide/tasks.md", "# Tasks\n")
        self.assertEqual(self.tree.problems(), [
            (f"{complete.COPY} a.b", "names the admin guide's Restarting the gateway section, which is not a heading "
                                     "in docs/admin-guide/")])
        self.tree.write("docs/admin-guide/more.md", "### Restarting the gateway ###\n")
        self.assertEqual(self.tree.problems(), [])

    def test_the_protocol_reference_names_every_frame_event_code_and_type(self):
        self.tree.write(complete.PROTOCOL_REFERENCE, REFERENCE.replace("| `busy` | later |\n", "")
                        .replace(", `RunId`", ""))
        self.assertEqual(self.tree.problems(), [
            (complete.PROTOCOL_REFERENCE, f"`RunId` is in {complete.SCHEMA} but the reference doesn't name it"),
            (complete.PROTOCOL_REFERENCE, f"`busy` is in {complete.SCHEMA} but the reference doesn't name it"),
        ])

    def test_a_listed_frame_the_schema_lacks_is_drift(self):
        self.tree.write(complete.PROTOCOL_REFERENCE, REFERENCE + "\n| `run.parked` | retired |\n")
        self.assertEqual(self.tree.problems(), [
            (f"{complete.PROTOCOL_REFERENCE}:13", f"`run.parked` is listed but {complete.SCHEMA} has no such frame, "
                                                  "event, code or type")])

    def test_without_a_schema_or_reference_rule_five_is_silent(self):
        (self.tree.root / complete.PROTOCOL_REFERENCE).unlink()
        self.assertEqual(complete.check_protocol_reference(self.tree.root), [])

    def test_a_failing_tree_prints_each_problem_and_exits_one(self):
        self.tree.write("docs/DEV-PLAN.md", "ADR-0010 and `FakeRelay`.\n")
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(complete.run_check(self.tree.root, DOCUMENTS), 1)
        lines = output.getvalue().splitlines()
        self.assertEqual(lines[-1], "G-09-complete: 2 missing document(s)")
        self.assertTrue(all(line.startswith("docs/DEV-PLAN.md:1: ") for line in lines[:-1]), lines)

    def test_the_repository_map_names_only_files_or_none(self):
        root = SOURCE.parent.parent
        for name, path in complete.NAMED_DOCUMENTS.items():
            with self.subTest(document=name):
                self.assertTrue(path is None or (root / path).is_file(), path)


if __name__ == "__main__":
    unittest.main()
