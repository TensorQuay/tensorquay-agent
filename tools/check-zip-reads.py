#!/usr/bin/env python3
"""G-05-zip (SEC-063): no Rust source under `crates/` names zip's extraction API (`ZipArchive::extract` and its
variants, and the stream reader). That API is where RUSTSEC-2026-0367, -0368 and -0369 were; `deny.toml` ignored them
from 17 Oct 2026 while calamine 0.36.1 held zip 8.6.0, and the ignore went on 21 Oct with calamine 0.37.0 and zip 9
(ADR-0017 point 9). We only read entries, so the ban stays: zip 9.0.1 to 9.0.3 fixed that API again. Comments are
skipped, and a plain `.extract(` counts only in a file that names `zip`, because axum and others have their own
`extract`. The stale check covers only these three IDs: if `deny.toml` ignores any of them again, the gate fails once
the lock holds no zip 8, or once calamine moves on from 0.36.1, the version that ignore was checked against. Stdlib
only."""

from pathlib import Path
import re
import sys
import tomllib

IGNORED = ("RUSTSEC-2026-0367", "RUSTSEC-2026-0368", "RUSTSEC-2026-0369")
# The calamine version whose own zip use was checked (reads only: `ZipArchive::new`, `by_name`) for the ignore.
CALAMINE = "0.36.1"
# zip's extraction API by name: these are refused in any file.
NAMED = re.compile(r"\bZipStreamReader\b|\bread::stream\b|\bread_zipfile_from_stream\b|\bZipArchive\s*::\s*extract"
                   r"|\bextract_(?:with_options|unwrapped_root_dir)\b")
# A method call `.extract(` (or `.extract::<T>(`), possibly split over lines: refused in a file that names zip.
METHOD = re.compile(r"\.\s*extract\s*(?:::\s*<[^>]*>\s*)?\(")
ZIP = re.compile(r"\bzip\b")


def code_of(text):
    """The text with each line's `//` comment removed, keeping the line breaks so offsets map to lines."""
    return "\n".join(line.split("//", 1)[0] for line in text.splitlines())


def extraction_calls(root):
    """`file:line: name` for every use of zip's extraction API in a Rust source under `crates/`."""
    found = []
    for path in sorted(Path(root, "crates").rglob("*.rs")):
        code = code_of(path.read_text())
        matches = list(NAMED.finditer(code))
        if ZIP.search(code):
            matches += METHOD.finditer(code)
        lines = set()
        for match in sorted(matches, key=lambda match: match.start()):
            line = code.count("\n", 0, match.end()) + 1
            if line in lines:
                continue
            lines.add(line)
            name = re.sub(r"\s+", "", match.group(0))
            found.append(f"{path.relative_to(root)}:{line}: {name} is zip's extraction API, where "
                         f"{', '.join(IGNORED)} were; read entries instead (ZipArchive::by_index / by_name)")
    return found


def read_toml(path):
    """The parsed file, or a problem naming it."""
    try:
        return tomllib.loads(path.read_text()), None
    except (OSError, tomllib.TOMLDecodeError) as error:
        return None, f"{path.name}: cannot be read as TOML ({type(error).__name__})"


def stale_ignore(root):
    """Problems when deny.toml still ignores the zip advisories that the lock no longer needs ignored."""
    deny, problem = read_toml(Path(root, "deny.toml"))
    if problem:
        return [problem]
    entries = deny.get("advisories", {}).get("ignore", [])
    ignored = sorted({entry if isinstance(entry, str) else entry.get("id") for entry in entries} & set(IGNORED))
    if not ignored:
        return []
    lock, problem = read_toml(Path(root, "Cargo.lock"))
    if problem:
        return [problem]
    versions = {}
    for package in lock.get("package", []):
        versions.setdefault(package.get("name"), set()).add(package.get("version", ""))
    if not any(version.split(".")[0] == "8" for version in versions.get("zip", ())):
        return [f"deny.toml: remove the ignore of {', '.join(ignored)}: the lock holds no zip 8 any more "
                f"(zip {', '.join(sorted(versions.get('zip', ()))) or 'none'}), so cargo deny can check it again; "
                "record its removal in ADR-0017 point 9"]
    calamine = sorted(versions.get("calamine", ()))
    if calamine != [CALAMINE]:
        return [f"Cargo.lock: calamine {', '.join(calamine) or 'none'} is in the lock, but the zip ignore was checked "
                f"against calamine {CALAMINE}. If it takes zip 9, bump zip and remove the ignore; if not, check that it "
                "still only reads its zip (no extract), then update CALAMINE in tools/check-zip-reads.py"]
    return []


def check_tree(root):
    """Every problem in the tree at `root`."""
    return extraction_calls(root) + stale_ignore(root)


def main():
    problems = check_tree(Path("."))
    for problem in problems:
        print(problem, file=sys.stderr)
    if problems:
        return 1
    print("zip reads: no extraction API under crates/; no stale zip advisory ignore")
    return 0


if __name__ == "__main__":
    sys.exit(main())
