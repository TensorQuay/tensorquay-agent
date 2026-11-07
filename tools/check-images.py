#!/usr/bin/env python3
"""G-05-images (SEC-030, ADR-0017 point 8): every third-party image a compose file under `deploy/` names is on the
approved list `deploy/third-party-images.toml`, pinned by the digest the list records, with an Apache-2.0-compatible
licence. Our own images (`tqa-*`, usually through `${TQA_*_IMAGE:-tqa-...}`) need no entry. An entry no compose file
uses is reported, so the list stays current. Compose files are read line by line for `image:`; stdlib only."""

from pathlib import Path
import re
import sys
import tomllib

LIST = Path("deploy/third-party-images.toml")
# Apache-2.0-compatible licences an image's own software may carry (SPDX ids, plus PostgreSQL's licence).
ALLOWED = {"Apache-2.0", "MIT", "MIT-0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "0BSD", "Unlicense", "CC0-1.0",
           "Zlib", "BSL-1.0", "PostgreSQL", "MPL-2.0"}
FIELDS = {"repository", "digest", "licence", "source"}
IMAGE_LINE = re.compile(r"""^\s*image:\s*["']?([^"'\s#]+)["']?\s*(#.*)?$""")
DIGEST = re.compile(r"@(sha256:[0-9a-f]{64})$")
SUBSTITUTED = re.compile(r"^\$\{[A-Z0-9_]+(?::-(.+))?\}$")


def repository(reference):
    """The repository of an image reference, with Docker Hub's default registry and namespace removed."""
    name = reference.split("@", 1)[0]
    last = name.rsplit("/", 1)[-1]
    if ":" in last:
        name = name[: len(name) - len(last)] + last.split(":", 1)[0]
    for prefix in ("docker.io/library/", "docker.io/", "library/"):
        if name.startswith(prefix):
            return name[len(prefix):]
    return name


def load_list(path):
    """The approved entries by repository, or the problems that make the list unusable."""
    try:
        document = tomllib.loads(path.read_text())
    except (OSError, tomllib.TOMLDecodeError) as error:
        return None, [f"{path}: cannot be read as TOML ({type(error).__name__})"]
    entries, problems = {}, []
    for index, entry in enumerate(document.get("image", [])):
        if set(entry) != FIELDS or not all(isinstance(entry[field], str) and entry[field] for field in FIELDS):
            problems.append(f"{path}: entry {index + 1} needs exactly {sorted(FIELDS)} as non-empty strings")
            continue
        if entry["repository"] in entries:
            problems.append(f"{path}: {entry['repository']} is listed twice")
            continue
        if entry["licence"] not in ALLOWED:
            problems.append(f"{path}: {entry['repository']} has licence {entry['licence']!r}, which is not an "
                            "approved Apache-2.0-compatible licence (source-available and enterprise licences never are)")
        entries[entry["repository"]] = entry
    if set(document) - {"image"}:
        problems.append(f"{path}: unknown top-level keys {sorted(set(document) - {'image'})}")
    return entries, problems


def compose_images(root):
    """(file, image reference) for every `image:` line in a compose file under `deploy/`."""
    for path in sorted(Path(root, "deploy").rglob("*.y*ml")):
        for line in path.read_text().splitlines():
            match = IMAGE_LINE.match(line)
            if match:
                yield path.relative_to(root).as_posix(), match.group(1)


def check_image(file, reference, entries, used):
    """The problem with one image reference, or None."""
    substituted = SUBSTITUTED.match(reference)
    if substituted:
        if substituted.group(1) is None:
            return f"{file}: {reference} has no default, so it cannot be checked"
        reference = substituted.group(1)
    name = repository(reference)
    if name.startswith("tqa-"):
        return None
    entry = entries.get(name)
    if entry is None:
        return f"{file}: {reference} ({name}) is not on the approved list {LIST}"
    used.add(name)
    pinned = DIGEST.search(reference)
    if not pinned or pinned.group(1) != entry["digest"]:
        return f"{file}: {reference} must be pinned by the digest {LIST} records for {name}"
    return None


def check_tree(root):
    """Every problem found, in a stable order; empty when the gate passes."""
    root = Path(root)
    entries, problems = load_list(root / LIST)
    if entries is None:
        return problems
    used = set()
    for file, reference in compose_images(root):
        problem = check_image(file, reference, entries, used)
        if problem:
            problems.append(problem)
    problems.extend(f"{LIST}: {name} is listed but no compose file uses it" for name in sorted(set(entries) - used))
    return problems


def main():
    problems = check_tree(Path(__file__).resolve().parents[1])
    for problem in problems:
        print(problem, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
