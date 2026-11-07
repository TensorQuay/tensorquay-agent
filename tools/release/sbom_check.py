"""The release gate on an artefact's SBOM (C1–C6 of the design): nothing in the image is left out, nothing is listed that
it does not hold, the project's linked code stays inside the licence policy, and the document is sound.

`problems` returns what failed, each line starting with its check; an empty list passes.
"""

from dataclasses import dataclass, field
import fnmatch
import os
from pathlib import Path
import re
import stat

import sbom_sources as sources

MAX_LISTED = 20

# Files that are not software: those the container engine writes into every exported filesystem (with the empty
# `/dev/console` mount point), and the two-line `exit 101` script the Debian base image build adds so package scripts
# start no services. A reviewed list, not a pattern.
NOT_SOFTWARE = frozenset({"/.dockerenv", "/etc/hostname", "/etc/hosts", "/etc/resolv.conf", "/dev/console",
                          "/usr/sbin/policy-rc.d"})
# dpkg's own maintainer scripts, owned by the packages they belong to.
DPKG_METADATA = "/var/lib/dpkg/"
# Merged /usr: dpkg may record a file under its pre-merge path.
MERGED_USR = ("/bin/", "/sbin/", "/lib/", "/lib32/", "/lib64/", "/libx32/")


@dataclass(frozen=True)
class Policy:
    """The licence policy for the project's linked code (ADR-0017): `deny.toml` and `web/licenses.json`."""
    allowed: frozenset
    exceptions: dict  # crate name -> licences allowed for that crate only
    forbidden_npm: list
    allowed_npm: frozenset = None


@dataclass(frozen=True)
class Declared:
    """A component the Dockerfile adds outside any package manager, with the image paths it owns (a trailing slash owns
    a directory's whole tree)."""
    package: sources.Package
    paths: list


@dataclass(frozen=True)
class Expectation:
    root: Path
    digest: str
    npm_dirs: list
    crates: list
    declared: list
    own_binaries: list
    policy: Policy
    # The npm trees the project links (the web app's); others, like the Node image's own npm, are listed, not gated.
    gated_npm_dirs: list = field(default=None)


def _split(expression, operator):
    """`expression` split on a top-level ` OR ` or ` AND `, outside parentheses."""
    parts, depth, start = [], 0, 0
    token = " {} ".format(operator)
    index = 0
    while index < len(expression):
        char = expression[index]
        depth += (char == "(") - (char == ")")
        if depth == 0 and expression.startswith(token, index):
            parts.append(expression[start:index])
            index += len(token)
            start = index
            continue
        index += 1
    parts.append(expression[start:])
    return [part.strip() for part in parts if part.strip()]


def _strip(term):
    """`term` without parentheses that enclose all of it: `(A OR B)` becomes `A OR B`; `(A) AND (B)` stays."""
    term = term.strip()
    while term.startswith("(") and term.endswith(")"):
        depth = 0
        for index, char in enumerate(term):
            depth += (char == "(") - (char == ")")
            if depth == 0 and index < len(term) - 1:
                return term
        term = term[1:-1].strip()
    return term


def allowed(expression, permitted, forbidden=()):
    """Whether an SPDX expression can be taken under `permitted` licences: one alternative whose every term is
    permitted (and none forbidden)."""
    expression = _strip(expression)
    for alternative in _split(expression, "OR"):
        terms = _split(_strip(alternative), "AND")
        if all(_term_allowed(term, permitted, forbidden) for term in terms):
            return True
    return False


def _term_allowed(term, permitted, forbidden):
    term = _strip(term)
    if " OR " in term or " AND " in term:
        return allowed(term, permitted, forbidden)
    if any(fnmatch.fnmatchcase(term, pattern) for pattern in forbidden):
        return False
    return term in permitted


def _listed(document, kind):
    prefix = "pkg:{}/".format(kind)
    found = {}
    for package in document.get("packages", [])[1:]:
        purls = [ref["referenceLocator"] for ref in package.get("externalRefs", []) if ref.get("referenceType") == "purl"]
        if purls and purls[0].startswith(prefix):
            found[(package["name"], package["versionInfo"])] = package
    return found


def _bounded(check, items, what):
    items = sorted(items)
    lines = ["{}: {}".format(check, item) for item in items[:MAX_LISTED]]
    if len(items) > MAX_LISTED:
        lines.append("{}: {} {} in all (first {} listed)".format(check, len(items), what, MAX_LISTED))
    return lines


def _c1_packages(document, expectation):
    installed = {(package.name, package.version) for package in sources.debian_packages(expectation.root)}
    listed = _listed(document, "deb")
    problems = _bounded("C1", ["{} {} is installed but not listed".format(*key) for key in installed - set(listed)],
                        "unlisted packages")
    problems += _bounded("C1", ["{} {} is listed but not installed".format(*key) for key in set(listed) - installed],
                         "packages listed in error")
    for (name, version), package in listed.items():
        if package.get("downloadLocation", "NOASSERTION") in ("", "NOASSERTION"):
            problems.append("C1: {} {} has no source location".format(name, version))
    return problems


def _owned_paths(expectation):
    owned = set(expectation.own_binaries)
    info = expectation.root / "var/lib/dpkg/info"
    if info.is_dir():
        for listing in info.glob("*.list"):
            for line in listing.read_text(encoding="utf-8", errors="replace").splitlines():
                path = line.strip()
                if path:
                    owned.add(path)
                    if path.startswith(MERGED_USR):
                        owned.add("/usr" + path)
    # A package's file moved by dpkg-divert stays the package's: var/lib/dpkg/diversions holds (from, to, package)
    # line triples, and a diversion counts only for a path a package owns (TOOL-U-064: the Desktop's xdotool).
    diversions = expectation.root / "var/lib/dpkg/diversions"
    if diversions.is_file():
        lines = diversions.read_text(encoding="utf-8", errors="replace").splitlines()
        for start in range(0, len(lines) - 2, 3):
            source, target = lines[start].strip(), lines[start + 1].strip()
            if source in owned and target:
                owned.add(target)
    return owned


def _c2_files(expectation):
    owned = _owned_paths(expectation)
    trees = [component_path for component in expectation.declared for component_path in component.paths]
    trees += ["/" + directory.strip("/") + "/" for directory in expectation.npm_dirs]
    unowned = []
    for directory, _, names in os.walk(expectation.root):
        for name in names:
            path = Path(directory) / name
            status = path.lstat()
            if not stat.S_ISREG(status.st_mode):
                continue
            image_path = "/" + str(path.relative_to(expectation.root))
            if image_path in NOT_SOFTWARE or image_path.startswith(DPKG_METADATA) or image_path in owned:
                continue
            if any(image_path == tree or (tree.endswith("/") and image_path.startswith(tree)) for tree in trees):
                continue
            with open(path, "rb") as handle:
                elf = handle.read(4) == b"\x7fELF"
            if elf or status.st_mode & 0o111:
                unowned.append(image_path)
    if not unowned:
        return []
    lines = _bounded("C2", ["{} is not owned by a package or a declared component".format(path) for path in unowned],
                     "unowned binaries")
    if len(unowned) > MAX_LISTED:
        lines[-1] = "C2: {} unowned binaries in all (first {} listed)".format(len(unowned), MAX_LISTED)
    return lines


def _c3_closures(document, expectation):
    problems = []
    expected_crates = {(crate.name, crate.version) for crate in expectation.crates}
    listed_crates = set(_listed(document, "cargo"))
    problems += _bounded("C3", ["crate {} {} is listed but not in the shipped closure".format(*key)
                                for key in listed_crates - expected_crates], "crates listed in error")
    problems += _bounded("C3", ["crate {} {} is shipped but not listed".format(*key)
                                for key in expected_crates - listed_crates], "unlisted crates")
    held = {(package.name, package.version) for package in sources.npm_packages(expectation.root, expectation.npm_dirs)}
    listed_npm = set(_listed(document, "npm"))
    problems += _bounded("C3", ["npm package {} {} is listed but not in the image".format(*key)
                                for key in listed_npm - held], "npm packages listed in error")
    problems += _bounded("C3", ["npm package {} {} is in the image but not listed".format(*key)
                                for key in held - listed_npm], "unlisted npm packages")
    return problems


def _c4_licences(expectation):
    policy, problems = expectation.policy, []
    for crate in expectation.crates:
        permitted = policy.allowed | frozenset(policy.exceptions.get(crate.name, ()))
        if not allowed(crate.license, permitted):
            problems.append("C4: crate {} {} is under {}, outside the licence policy".format(
                crate.name, crate.version, crate.license))
    gated = expectation.gated_npm_dirs if expectation.gated_npm_dirs is not None else expectation.npm_dirs
    for package in sources.npm_packages(expectation.root, gated):
        if not allowed(package.license, policy.allowed_npm or policy.allowed, policy.forbidden_npm):
            problems.append("C4: npm package {} {} is under {}, outside the licence policy".format(
                package.name, package.version, package.license))
    return problems


def _c5_document(document):
    problems = []
    for key, value in (("spdxVersion", "SPDX-2.3"), ("dataLicense", "CC0-1.0"), ("SPDXID", "SPDXRef-DOCUMENT")):
        if document.get(key) != value:
            problems.append("C5: {} is not {}".format(key, value))
    if not document.get("documentNamespace") or not document.get("creationInfo", {}).get("created"):
        problems.append("C5: the document has no namespace or creation time")
    packages = document.get("packages", [])
    ids = [package.get("SPDXID") for package in packages]
    if len(ids) != len(set(ids)):
        problems.append("C5: SPDX ids repeat")
    for package in packages:
        missing = [key for key in ("SPDXID", "name", "versionInfo", "downloadLocation", "licenseDeclared")
                   if not package.get(key)]
        if missing:
            problems.append("C5: {} lacks {}".format(package.get("name", "a package"), ", ".join(missing)))
    relationships = document.get("relationships", [])
    described = [item["relatedSpdxElement"] for item in relationships
                 if item.get("spdxElementId") == "SPDXRef-DOCUMENT" and item.get("relationshipType") == "DESCRIBES"]
    if len(described) != 1 or not packages or described[0] != packages[0].get("SPDXID"):
        return problems + ["C5: the document does not describe exactly its first package"]
    contained = {item["relatedSpdxElement"] for item in relationships
                 if item.get("spdxElementId") == described[0] and item.get("relationshipType") == "CONTAINS"}
    orphans = [package["name"] for package in packages[1:] if package.get("SPDXID") not in contained]
    problems += _bounded("C5", ["{} is not contained by the described artefact".format(name) for name in orphans],
                         "uncontained packages")
    explained = {item.get("licenseId") for item in document.get("hasExtractedLicensingInfos", [])}
    references = set()
    for package in packages:
        references.update(re.findall(r"LicenseRef-[A-Za-z0-9.-]+", package.get("licenseDeclared", "")))
    problems += _bounded("C5", ["{} has no extracted licence text".format(ref) for ref in references - explained],
                         "unexplained licence references")
    return problems


def _c6_identity(document, expectation):
    packages = document.get("packages", [])
    checksums = packages[0].get("checksums", []) if packages else []
    value = next((item["checksumValue"] for item in checksums if item.get("algorithm") == "SHA256"), "")
    if "sha256:" + value != expectation.digest:
        return ["C6: the document describes sha256:{}, not the released {}".format(value, expectation.digest)]
    return []


def document_problems(document):
    """C5 alone, for an artefact with no filesystem to read (the kernel)."""
    return _c5_document(document)


def problems(document, expectation):
    """Every check's findings, in order; an empty list passes."""
    return (_c1_packages(document, expectation) + _c2_files(expectation) + _c3_closures(document, expectation)
            + _c4_licences(expectation) + _c5_document(document) + _c6_identity(document, expectation))
