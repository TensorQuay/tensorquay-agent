#!/usr/bin/env python3
"""G-09-complete: every document the plans name exists (PRD NFR-8; MS5's "docs complete").

G-09 checks that links resolve and test IDs are defined; this gate checks that what the plans promise in words is
there. Four rules, each reported at the line that names the missing thing:
1. An ADR cited by the plans or by another ADR has its record, `docs/adr/ADR-NNNN-<title>.md`.
2. A testkit fake the plans name in backticks (`FakeClock`) is defined in a crate: a public struct, enum, trait or
   type, or a public re-export.
3. A document the plans name (the install guide, the protocol reference...) is a key of NAMED_DOCUMENTS, and the
   file that key gives exists. None records that no such file exists yet, and fails.
4. A guide section the web copy names ("the admin guide's Environments section") is a heading in that guide.
5. The protocol reference names, in backticks, every definition of the protocol's JSON Schema and every client frame,
   server frame, event and error code in it; and each frame, event or code its tables list (a backticked first cell)
   is in the schema, so the page can't drift from the protocol either way.
"""

import json
import os
from pathlib import Path
import re
import sys


PLANS = ("docs/PRD.md", "docs/ARCHITECTURE.md", "docs/DEV-PLAN.md", "docs/TEST-PLAN.md", "docs/DEV-GUIDELINES.md",
         "docs/ENGINEERING-GUARDRAILS.md", "docs/UI-GUIDELINES.md")
ADR_DIRECTORY = "docs/adr"
COPY = "web/src/i18n/locales/en.json"
GUIDES = {"admin": "docs/admin-guide", "user": "docs/user-guide"}
SCHEMA = "crates/tqa-types/schema/tqa-protocol-v1.json"
PROTOCOL_REFERENCE = "docs/protocol-reference.md"
TAGGED_UNIONS = ("ClientFrame", "ServerFrame", "Event")
SKIP_DIRECTORIES = {".git", "target", "node_modules", ".next", "generated"}

# Each document the plans name, by the words they use, and the file that is it. None: no such file exists yet.
NAMED_DOCUMENTS = {
    "admin guide": "docs/admin-guide/README.md",
    "api reference": "crates/tqa-gateway/openapi.json",
    "contributor guide": "CONTRIBUTING.md",
    "install guide": "deploy/README.md",
    "protocol reference": "docs/protocol-reference.md",
    "user guide": "docs/user-guide/README.md",
}

ADR = re.compile(r"\bADR-(\d{4})\b")
ADR_FILE = re.compile(r"^ADR-(\d{4})-[^/]+\.md$")
FAKE = re.compile(r"`(Fake[A-Z][A-Za-z0-9]*)`")
DOCUMENT_NAME = re.compile(r"\b(?:(install|admin|user|contributor) guide|(protocol|api) reference)\b", re.I)
DEFINITION = re.compile(r"^\s*pub\s+(?:struct|enum|trait|type)\s+([A-Za-z0-9_]+)", re.M)
REEXPORT = re.compile(r"^\s*pub\s+use\s+[^;]*;", re.M | re.S)
GUIDE_SECTION = re.compile(r"\bthe (admin|user) guide's (.+?) section\b")
HEADING = re.compile(r"^#{1,6}\s+(.+?)\s*#*\s*$", re.M)
BACKTICKED = re.compile(r"`([^`\s]+)`")
FIRST_CELL = re.compile(r"^\|\s*`([^`]+)`\s*\|", re.M)


def lines_of(root, name):
    """Yield (line number, text) of a file under root, or nothing when it is missing."""
    path = root / name
    if path.is_file():
        yield from enumerate(path.read_text(encoding="utf-8").splitlines(), 1)


def adr_files(root):
    directory = root / ADR_DIRECTORY
    names = sorted(path.name for path in directory.iterdir()) if directory.is_dir() else []
    return {match[1] for match in map(ADR_FILE.match, names) if match}


def check_adrs(root):
    """Rule 1: one problem per missing record, at its first citation, with the count of citations."""
    present, cited = adr_files(root), {}
    directory = root / ADR_DIRECTORY
    records = sorted(path.name for path in directory.glob("*.md")) if directory.is_dir() else []
    sources = list(PLANS) + [f"{ADR_DIRECTORY}/{name}" for name in records]
    for name in sources:
        for number, text in lines_of(root, name):
            for match in ADR.finditer(text):
                cited.setdefault(match[1], []).append(f"{name}:{number}")
    return [(places[0], f"ADR-{adr} is cited ({len(places)} times) but {ADR_DIRECTORY}/ADR-{adr}-*.md does not exist")
            for adr, places in sorted(cited.items()) if adr not in present]


def rust_names(root):
    """Every public type name and every name a public `use` re-exports, in the crates."""
    names = set()
    for directory, directories, files in os.walk(root / "crates"):
        directories[:] = sorted(name for name in directories if name not in SKIP_DIRECTORIES)
        for file in files:
            if file.endswith(".rs"):
                source = (Path(directory) / file).read_text(encoding="utf-8")
                names.update(DEFINITION.findall(source))
                for statement in REEXPORT.findall(source):
                    names.update(re.findall(r"\b(?:as\s+)?([A-Z][A-Za-z0-9_]*)\b", statement))
    return names


def check_fakes(root):
    """Rule 2: one problem per fake the plans name and no crate defines, at its first mention."""
    defined, named = rust_names(root), {}
    for name in PLANS:
        for number, text in lines_of(root, name):
            for fake in FAKE.findall(text):
                named.setdefault(fake, f"{name}:{number}")
    return [(place, f"`{fake}` is named as a testkit fake but no crate defines or re-exports it")
            for fake, place in sorted(named.items()) if fake not in defined]


def check_named_documents(root, documents):
    """Rule 3: every document name the plans use is mapped, and its file exists."""
    problems, reported = [], set()
    for name in PLANS:
        for number, text in lines_of(root, name):
            for match in DOCUMENT_NAME.finditer(text):
                key = match[0].lower()
                if key in reported:
                    continue
                reported.add(key)
                if key not in documents:
                    problems.append((f"{name}:{number}", f"the {key} is named but NAMED_DOCUMENTS has no entry for it"))
                elif documents[key] is None:
                    problems.append((f"{name}:{number}", f"the {key} is named but no file is it yet"))
                elif not (root / documents[key]).is_file():
                    problems.append((f"{name}:{number}", f"the {key} is named but {documents[key]} does not exist"))
    return problems


def guide_headings(root, guide):
    directory, headings = root / GUIDES[guide], set()
    for path in sorted(directory.glob("*.md")) if directory.is_dir() else []:
        headings.update(HEADING.findall(path.read_text(encoding="utf-8")))
    return headings


def check_guide_sections(root):
    """Rule 4: a guide section the English copy names is a heading in that guide."""
    path = root / COPY
    copy = json.loads(path.read_text(encoding="utf-8")) if path.is_file() else {}
    problems = []
    for key, text in sorted(flatten(copy)):
        for guide, section in GUIDE_SECTION.findall(text):
            if section not in guide_headings(root, guide):
                problems.append((f"{COPY} {key}", f"names the {guide} guide's {section} section, which is not a "
                                                  f"heading in {GUIDES[guide]}/"))
    return problems


def schema_names(schema):
    """Every definition name, and every frame, event and error code value, in the protocol's JSON Schema."""
    definitions = schema.get("definitions", {})
    names = set(definitions)
    for union in TAGGED_UNIONS:
        for variant in definitions.get(union, {}).get("oneOf", []):
            names.add(variant.get("properties", {}).get("type", {}).get("const"))
    for variant in definitions.get("ErrorCode", {}).get("oneOf", []):
        names.update(variant.get("enum") or [variant.get("const")])
    return names - {None}


def check_protocol_reference(root):
    """Rule 5: the reference names everything in the schema, and its tables name nothing the schema lacks."""
    schema, page = root / SCHEMA, root / PROTOCOL_REFERENCE
    if not schema.is_file() or not page.is_file():
        return []  # rule 3 reports a missing reference; a tree without the schema has no protocol to describe
    names, text = schema_names(json.loads(schema.read_text(encoding="utf-8"))), page.read_text(encoding="utf-8")
    listed = set(BACKTICKED.findall(text))
    problems = [(PROTOCOL_REFERENCE, f"`{name}` is in {SCHEMA} but the reference doesn't name it")
                for name in sorted(names - listed)]
    for match in FIRST_CELL.finditer(text):
        if match[1] not in names:
            line = text.count("\n", 0, match.start()) + 1
            problems.append((f"{PROTOCOL_REFERENCE}:{line}", f"`{match[1]}` is listed but {SCHEMA} has no such "
                                                               "frame, event, code or type"))
    return problems


def flatten(tree, prefix=""):
    """(dotted key, text) for every string in the copy catalogue."""
    if isinstance(tree, str):
        yield prefix, tree
    elif isinstance(tree, dict):
        for key, value in tree.items():
            yield from flatten(value, f"{prefix}.{key}" if prefix else key)


def run_check(root, documents=None):
    """Print each problem and return the gate's exit status."""
    documents = NAMED_DOCUMENTS if documents is None else documents
    problems = (check_adrs(root) + check_fakes(root) + check_named_documents(root, documents)
                + check_guide_sections(root) + check_protocol_reference(root))
    for place, message in problems:
        print(f"{place}: {message}")
    print(f"G-09-complete: {len(problems)} missing document(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(run_check(Path(__file__).resolve().parent.parent))
