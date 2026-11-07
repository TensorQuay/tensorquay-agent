"""What an image contains, read offline from its exported filesystem and the build's own metadata (ADR-0017 point 7).

Each reader returns `Package` records: the exact version, the location of the matching source, and the licence as an
exact SPDX expression or a `LicenseRef` pointing at the package's own notice. A licence is never guessed.
"""

from dataclasses import dataclass
import json
from pathlib import Path
import re
from urllib.parse import quote

PROJECT_SOURCE = "https://github.com/TensorQuay/tensorquay-agent"

# Debian's short licence names that name exactly one SPDX licence (DEP-5 §7). Anything else stays a reference.
DEBIAN_TO_SPDX = {
    "Apache-2.0": "Apache-2.0",
    "Artistic-2.0": "Artistic-2.0",
    "BSD-2-clause": "BSD-2-Clause",
    "BSD-3-clause": "BSD-3-Clause",
    "BSD-4-clause": "BSD-4-Clause",
    "BSL-1.0": "BSL-1.0",
    "CC0-1.0": "CC0-1.0",
    "Expat": "MIT",
    "MIT": "MIT",
    "GFDL-1.2+": "GFDL-1.2-or-later",
    "GFDL-1.3+": "GFDL-1.3-or-later",
    "GPL-1+": "GPL-1.0-or-later",
    "GPL-2": "GPL-2.0-only",
    "GPL-2+": "GPL-2.0-or-later",
    "GPL-3": "GPL-3.0-only",
    "GPL-3+": "GPL-3.0-or-later",
    "ISC": "ISC",
    "LGPL-2": "LGPL-2.0-only",
    "LGPL-2+": "LGPL-2.0-or-later",
    "LGPL-2.1": "LGPL-2.1-only",
    "LGPL-2.1+": "LGPL-2.1-or-later",
    "LGPL-3": "LGPL-3.0-only",
    "LGPL-3+": "LGPL-3.0-or-later",
    "MPL-2.0": "MPL-2.0",
    "Zlib": "Zlib",
    "zlib": "Zlib",
    # SPDX ids some packagers write directly (and SPDX's deprecated "+" spellings, which mean "-or-later").
    "Beerware": "Beerware",
    "BSD-2-Clause": "BSD-2-Clause",
    "BSD-3-Clause": "BSD-3-Clause",
    "curl": "curl",
    "FSFAP": "FSFAP",
    "FSFUL": "FSFUL",
    "FSFULLR": "FSFULLR",
    "GPL-2.0+": "GPL-2.0-or-later",
    "GPL-3.0+": "GPL-3.0-or-later",
    "LGPL-2.1+": "LGPL-2.1-or-later",
    "X11": "X11",
    # Debian names of SPDX licences under another spelling.
    "GFDL-NIV-1.3+": "GFDL-1.3-no-invariants-or-later",
    "OpenLDAP-2.8": "OLDAP-2.8",
}


@dataclass(frozen=True)
class Package:
    """One component of an artefact."""
    name: str
    version: str
    kind: str  # "deb", "npm", "cargo" or "generic"
    license: str
    download_location: str
    sha256: str = ""
    source_name: str = ""
    source_version: str = ""
    architecture: str = ""
    # Where the licence text is, for a `LicenseRef` (a path in the image, or a URL).
    license_notice: str = ""

    @property
    def purl(self):
        if self.kind == "deb":
            return "pkg:deb/debian/{}@{}?arch={}".format(self.name, quote(self.version, safe=""), self.architecture)
        if self.kind == "npm":
            return "pkg:npm/{}@{}".format(quote(self.name, safe="/"), self.version)
        if self.kind == "cargo":
            return "pkg:cargo/{}@{}".format(self.name, self.version)
        return "pkg:generic/{}@{}".format(quote(self.name, safe=""), quote(self.version, safe=""))


def _stanzas(text):
    """RFC 822-style paragraphs: dpkg's status file and DEP-5 copyright files."""
    for block in re.split(r"\n\s*\n", text.strip()):
        fields, key = {}, None
        for line in block.splitlines():
            if line[:1] in (" ", "\t") and key:
                fields[key] += "\n" + line.strip()
            elif ":" in line:
                key, _, value = line.partition(":")
                key = key.strip().lower()
                fields[key] = value.strip()
        if fields:
            yield fields


def _spdx_term(term):
    """One Debian licence name as an SPDX id, or None when it is not exactly one."""
    return DEBIAN_TO_SPDX.get(term.strip())


def _spdx_line(line):
    """A DEP-5 `License:` short line as an SPDX expression, or None when any part is not exact (an exception, a
    custom name, prose)."""
    if " with " in line.lower():
        return None
    alternatives = [part for part in re.split(r"\s+or\s+", line.strip(), flags=re.IGNORECASE) if part]
    mapped = []
    for alternative in alternatives:
        terms = [part for part in re.split(r"\s+and\s+|,", alternative, flags=re.IGNORECASE) if part.strip()]
        ids = [_spdx_term(term) for term in terms]
        if not ids or None in ids:
            return None
        mapped.append(" AND ".join(sorted(set(ids))))
    if not mapped:
        return None
    if len(mapped) == 1:
        return mapped[0]
    return "(" + " OR ".join(sorted(mapped)) + ")"


def debian_license(package, text):
    """A package's declared licence from its copyright file: the exact SPDX expression of every `Files` stanza's
    `License:` in a machine-readable (DEP-5) file, joined by AND, or `LicenseRef-debian-<package>` for anything else."""
    # SPDX ids hold letters, digits, "." and "-" only: `libstdc++6` becomes `libstdcplusplus6`.
    reference = "LicenseRef-debian-" + re.sub(r"[^A-Za-z0-9.-]", "-", package.replace("+", "plus"))
    first = text.splitlines()[0] if text else ""
    if not first.startswith("Format:") or "copyright-format/1.0" not in first:
        return reference
    lines = sorted({fields["license"].splitlines()[0].strip() for fields in _stanzas(text)
                    if "files" in fields and fields.get("license")})
    expressions = [_spdx_line(line) for line in lines]
    if not expressions or None in expressions:
        return reference
    return " AND ".join(sorted(set(expressions)))


def _status_files(root):
    main = root / "var/lib/dpkg/status"
    files = [main] if main.is_file() else []
    # Distroless images keep one status file per package.
    extra = root / "var/lib/dpkg/status.d"
    if extra.is_dir():
        files += sorted(path for path in extra.iterdir() if path.is_file() and not path.name.endswith(".md5sums"))
    return files


def debian_packages(root):
    """The installed Debian packages of the image tree at `root`, with their source packages and licences."""
    root = Path(root)
    packages = []
    for path in _status_files(root):
        for fields in _stanzas(path.read_text(encoding="utf-8", errors="replace")):
            status = fields.get("status", "install ok installed")
            if "package" not in fields or not status.endswith(" installed"):
                continue
            name, version = fields["package"], fields["version"]
            source = fields.get("source", name)
            source_name, _, rest = source.partition(" ")
            source_version = rest.strip().strip("()") or version
            notice_path = "/usr/share/doc/{}/copyright".format(name)
            notice = root / notice_path.lstrip("/")
            if not notice.is_file():
                raise ValueError("no licence notice for Debian package " + name)
            text = notice.read_text(encoding="utf-8", errors="replace")
            packages.append(Package(
                name=name, version=version, kind="deb", license=debian_license(name, text),
                download_location="https://snapshot.debian.org/package/{}/{}/".format(
                    quote(source_name), quote(source_version)),
                source_name=source_name, source_version=source_version,
                architecture=fields.get("architecture", ""), license_notice=notice_path))
    return sorted(packages, key=lambda package: (package.name, package.version))


def _package_roots(directory):
    """`package.json` files that are package roots: directly in a `node_modules`, or in a scope directory in one."""
    for path in directory.rglob("package.json"):
        parent = path.parent
        if parent.parent.name == "node_modules" or (
                parent.parent.name.startswith("@") and parent.parent.parent.name == "node_modules"):
            yield path


def npm_packages(root, directories):
    """The npm packages installed under `directories` (relative to `root`), with their registry tarballs."""
    root = Path(root)
    found = {}
    for directory in directories:
        top = root / directory
        if not top.is_dir():
            continue
        for path in sorted(_package_roots(top)):
            metadata = json.loads(path.read_text(encoding="utf-8"))
            name, version = metadata.get("name"), metadata.get("version")
            if not name or not version:
                continue
            license_name = metadata.get("license")
            if license_name is None and isinstance(metadata.get("licenses"), list):
                license_name = " OR ".join(item.get("type", "") for item in metadata["licenses"])
            if not isinstance(license_name, str) or not license_name.strip():
                raise ValueError("no licence for npm package {} at {}".format(name, path.relative_to(root)))
            found[(name, version)] = Package(
                name=name, version=version, kind="npm", license=license_name.strip(),
                download_location="https://registry.npmjs.org/{}/-/{}-{}.tgz".format(
                    name, name.split("/")[-1], version))
    return [found[key] for key in sorted(found)]


def _lock_checksums(lock):
    checksums = {}
    for block in re.split(r"\n\s*\n", lock):
        fields = dict(re.findall(r'^(name|version|checksum) = "([^"]*)"', block, re.MULTILINE))
        if "checksum" in fields:
            checksums[(fields["name"], fields["version"])] = fields["checksum"]
    return checksums


def cargo_license(expression):
    """A crate's licence as an SPDX expression: Cargo reads the deprecated `/` separator (`MIT/Apache-2.0`) as OR."""
    return " OR ".join(part.strip() for part in expression.split("/")) if "/" in expression else expression


def cargo_packages(metadata, tree, lock):
    """The crates of one shipped binary: the closure `cargo tree -e normal --prefix none` printed (`tree`), described by
    `cargo metadata` (`metadata`) and `Cargo.lock` (`lock`). Workspace crates are the project's own."""
    described = {(package["name"], package["version"]): package for package in metadata["packages"]}
    checksums = _lock_checksums(lock)
    closure = set()
    for line in tree.splitlines():
        match = re.match(r"^(\S+) v(\S+)", line.strip())
        if match:
            closure.add((match.group(1), match.group(2)))
    packages = []
    for name, version in sorted(closure):
        package = described.get((name, version))
        if package is None:
            raise ValueError("crate {} {} is not in the cargo metadata".format(name, version))
        if not package.get("license"):
            raise ValueError("crate {} {} has no licence expression".format(name, version))
        own = package.get("source") is None
        packages.append(Package(
            name=name, version=version, kind="cargo", license=cargo_license(package["license"]),
            download_location=PROJECT_SOURCE if own else
            "https://static.crates.io/crates/{0}/{0}-{1}.crate".format(name, version),
            sha256="" if own else checksums.get((name, version), "")))
    return packages
