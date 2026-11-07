#!/usr/bin/env python3
"""The release vulnerability gate: the known vulnerabilities of what the release SBOMs list, failing on HIGH.

    python3 tools/release/sbom_vulns.py --sbom dist/<version>/sbom [--tracker <debian tracker json>]

Crates, npm packages and the Go toolchain are looked up in OSV; Debian packages in the Debian security tracker, by
source package, in the image's own release. It writes `vulns.json` and `vulns.md` next to the SBOMs and exits 1 when
any finding is HIGH. Levels:
- HIGH: a fix exists and the advisory is high or critical (its reviewed label, else its CVSS v3 score); or Debian has
  a security update (the release's `-security` suite) newer than the installed version, which a rebuild at a newer
  snapshot picks up.
- MEDIUM: a fix exists at a lower or unknown severity, or Debian fixed it in a point release.
- OPEN: no fix exists; reported with its severity, never failing (there is nothing to update to).
Components no database covers (Node, Firecracker, the kernel, the Rust standard library, musl) are listed as unchecked.
"""

import argparse
import json
import math
from pathlib import Path
import re
import sys
import tempfile
import urllib.request

OSV = "https://api.osv.dev/v1"
TRACKER = "https://security-tracker.debian.org/tracker/data/json"
RELEASES = {"12": "bookworm", "13": "trixie"}
LEVELS = ("HIGH", "MEDIUM", "OPEN")
BATCH = 500
TIMEOUT = 120
# Reviewed: a source package shipped only as these binaries is not what the tracker's issues are about. The kernel's
# headers (`linux-libc-dev`) are not a kernel; no kernel runs in a container, and the guest kernel has its own SBOM.
NOT_APPLICABLE = {"linux": {"linux-libc-dev"}}


# --- Debian version order (Debian Policy 5.6.12), so the gate needs no dpkg ---

def _order(character):
    if character == "~":
        return -1
    if character.isalpha():
        return ord(character)
    return ord(character) + 256 if character else 0


def _compare_part(left, right):
    while left or right:
        left_text = re.match(r"\D*", left).group()
        right_text = re.match(r"\D*", right).group()
        for index in range(max(len(left_text), len(right_text))):
            a = _order(left_text[index] if index < len(left_text) else "")
            b = _order(right_text[index] if index < len(right_text) else "")
            if a != b:
                return a - b
        left, right = left[len(left_text):], right[len(right_text):]
        left_digits = re.match(r"\d*", left).group()
        right_digits = re.match(r"\d*", right).group()
        if int(left_digits or 0) != int(right_digits or 0):
            return int(left_digits or 0) - int(right_digits or 0)
        left, right = left[len(left_digits):], right[len(right_digits):]
    return 0


def _split(version):
    epoch, _, rest = version.rpartition(":") if ":" in version else ("0", "", version)
    upstream, _, revision = rest.rpartition("-") if "-" in rest else (rest, "", "0")
    return int(epoch or 0), upstream, revision


def debian_compare(left, right):
    """Negative, zero or positive as `left` sorts before, equal to or after `right`."""
    left_epoch, left_upstream, left_revision = _split(left)
    right_epoch, right_upstream, right_revision = _split(right)
    if left_epoch != right_epoch:
        return left_epoch - right_epoch
    return _compare_part(left_upstream, right_upstream) or _compare_part(left_revision, right_revision)


# --- Severity ---

_CVSS3 = {"AV": {"N": 0.85, "A": 0.62, "L": 0.55, "P": 0.2}, "AC": {"L": 0.77, "H": 0.44},
          "UI": {"N": 0.85, "R": 0.62}, "C": {"H": 0.56, "L": 0.22, "N": 0.0}}
_PR = {"U": {"N": 0.85, "L": 0.62, "H": 0.27}, "C": {"N": 0.85, "L": 0.68, "H": 0.5}}


def _roundup(value):
    scaled = round(value * 100000)
    return scaled / 100000.0 if scaled % 10000 == 0 else (math.floor(scaled / 10000) + 1) / 10.0


def cvss3_score(vector):
    """The CVSS v3.0/v3.1 base score of `vector`, or None when it is not a v3 vector."""
    if not vector.startswith(("CVSS:3.0/", "CVSS:3.1/")):
        return None
    metrics = dict(part.split(":", 1) for part in vector.split("/")[1:])
    scope = metrics["S"]
    impact_base = 1 - ((1 - _CVSS3["C"][metrics["C"]]) * (1 - _CVSS3["C"][metrics["I"]])
                       * (1 - _CVSS3["C"][metrics["A"]]))
    if scope == "U":
        impact = 6.42 * impact_base
    else:
        impact = 7.52 * (impact_base - 0.029) - 3.25 * (impact_base - 0.02) ** 15
    exploitability = (8.22 * _CVSS3["AV"][metrics["AV"]] * _CVSS3["AC"][metrics["AC"]] * _PR[scope][metrics["PR"]]
                      * _CVSS3["UI"][metrics["UI"]])
    if impact <= 0:
        return 0.0
    if scope == "U":
        return _roundup(min(impact + exploitability, 10))
    return _roundup(min(1.08 * (impact + exploitability), 10))


def _band(score):
    return "CRITICAL" if score >= 9 else "HIGH" if score >= 7 else "MEDIUM" if score >= 4 else "LOW"


def advisory_severity(record):
    """The advisory's reviewed label (GitHub's MODERATE reads as MEDIUM), else its CVSS v3 band, else UNKNOWN."""
    label = str(record.get("database_specific", {}).get("severity", "")).upper()
    if label:
        return "MEDIUM" if label == "MODERATE" else label
    scores = [cvss3_score(entry.get("score", "")) for entry in record.get("severity", [])]
    scores = [score for score in scores if score is not None]
    return _band(max(scores)) if scores else "UNKNOWN"


def _fixed_versions(record):
    return sorted({event["fixed"] for affected in record.get("affected", []) for rng in affected.get("ranges", [])
                   for event in rng.get("events", []) if "fixed" in event})


def osv_level(record):
    if not _fixed_versions(record):
        return "OPEN"
    return "HIGH" if advisory_severity(record) in ("CRITICAL", "HIGH") else "MEDIUM"


# --- Reading the SBOM ---

def _purl(package):
    for reference in package.get("externalRefs", []):
        if reference.get("referenceType") == "purl":
            return reference["referenceLocator"]
    return ""


def _sources(document):
    """Debian source package -> (source version, the binary packages built from it)."""
    found = {}
    for package in document["packages"]:
        if not _purl(package).startswith("pkg:deb/"):
            continue
        match = re.match(r"built package from: (\S+) (\S+)", package.get("sourceInfo", ""))
        source, version = match.groups() if match else (package["name"], package["versionInfo"])
        found.setdefault(source, (version, set()))[1].add(package["name"])
    return found


def debian_release(document):
    """The image's Debian release codename from its base-files, None when it has no Debian packages."""
    for package in document["packages"]:
        if package["name"] == "base-files" and _purl(package).startswith("pkg:deb/"):
            major = package["versionInfo"].split(".")[0]
            if major not in RELEASES:
                raise ValueError("unknown Debian {} (base-files {})".format(major, package["versionInfo"]))
            return RELEASES[major]
    return None


def osv_queries(document):
    """(key, OSV query) for each crate, npm package and Go toolchain; and the components no database covers."""
    queries, unchecked = [], []
    for package in document["packages"][1:]:
        locator = _purl(package)
        if locator.startswith(("pkg:cargo/", "pkg:npm/")):
            key = locator.split("?")[0]
            queries.append((key, {"package": {"purl": key}}))
        elif locator.startswith("pkg:deb/"):
            continue
        elif package["name"] == "go":
            queries.append(("go-stdlib@" + package["versionInfo"],
                            {"package": {"name": "stdlib", "ecosystem": "Go"}, "version": package["versionInfo"]}))
        elif package["name"] != "tensorquay-agent":
            unchecked.append("{} {}".format(package["name"], package["versionInfo"]))
    return queries, unchecked


# --- Findings ---

def debian_findings(image, document, tracker):
    release = debian_release(document)
    if release is None:
        return []
    findings = []
    for source, (version, binaries) in sorted(_sources(document).items()):
        if binaries <= NOT_APPLICABLE.get(source, set()):
            continue
        for cve, entry in sorted(tracker.get(source, {}).items()):
            status = entry.get("releases", {}).get(release)
            if not status:
                continue
            urgency = str(status.get("urgency", "")).upper()
            finding = {"image": image, "package": source, "version": version, "id": cve, "fixed": None,
                       "severity": urgency or "UNKNOWN", "summary": entry.get("description", "")[:160]}
            if status["status"] == "open":
                findings.append(dict(finding, level="OPEN"))
            elif status["status"] == "resolved":
                fixed = status.get("fixed_version", "0")
                if fixed in ("", "0") or debian_compare(fixed, version) <= 0:
                    continue
                security = status.get("repositories", {}).get(release + "-security") == fixed
                level = "HIGH" if security or urgency == "HIGH" else "MEDIUM"
                findings.append(dict(finding, level=level, fixed=fixed))
    return findings


def report(findings, unchecked):
    counts = {}
    for finding in findings:
        per_image = counts.setdefault(finding["image"], {})
        per_image[finding["level"]] = per_image.get(finding["level"], 0) + 1
    return {"passed": not any(finding["level"] == "HIGH" for finding in findings), "counts": counts,
            "findings": sorted(findings, key=lambda f: (LEVELS.index(f["level"]), f["image"], f["package"], f["id"])),
            "unchecked": unchecked}


def markdown(result):
    lines = ["# Release vulnerability gate", "", "**{}**: {} HIGH.".format(
        "Passed" if result["passed"] else "FAILED",
        sum(counts.get("HIGH", 0) for counts in result["counts"].values())), "",
        "| Image | HIGH | MEDIUM | OPEN |", "| --- | --- | --- | --- |"]
    lines += ["| {} | {} | {} | {} |".format(image, *(counts.get(level, 0) for level in LEVELS))
              for image, counts in sorted(result["counts"].items())]
    lines += ["", "## HIGH and MEDIUM", "", "| Image | Package | Installed | Advisory | Level | Fixed in | Summary |",
              "| --- | --- | --- | --- | --- | --- | --- |"]
    lines += ["| {image} | {package} | {version} | {id} | {level} | {fixed} | {summary} |".format(
        **dict(f, summary=f["summary"].replace("|", "/").replace("\n", " ")[:100]))
        for f in result["findings"] if f["level"] != "OPEN"]
    opened = [f for f in result["findings"] if f["level"] == "OPEN" and f["severity"] in ("CRITICAL", "HIGH")]
    lines += ["", "## OPEN at high or critical severity (no fix exists; accept or mitigate)", ""]
    lines += ["- {image}: {package} {version}, {id} ({severity})".format(**f) for f in opened] or ["None."]
    lines += ["", "## Not checked by any database", ""]
    lines += ["- {}: {}".format(image, ", ".join(names)) for image, names in sorted(result["unchecked"].items())
              if names] or ["None."]
    return "\n".join(lines) + "\n"


# --- Network (thin; the logic above is what the tests cover) ---

def _post(url, body):
    request = urllib.request.Request(url, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
        return json.load(response)


def _get(url):
    with urllib.request.urlopen(url, timeout=TIMEOUT) as response:
        return json.load(response)


def osv_findings(image, document, records):
    queries, _ = osv_queries(document)
    hits = {}
    for start in range(0, len(queries), BATCH):
        batch = queries[start:start + BATCH]
        results = _post(OSV + "/querybatch", {"queries": [query for _, query in batch]})["results"]
        for (key, _), result in zip(batch, results):
            hits[key] = [vuln["id"] for vuln in result.get("vulns", [])]
    findings = []
    for key, ids in sorted(hits.items()):
        name, _, version = key.rpartition("@")
        for vid in ids:
            if vid not in records:
                records[vid] = _get(OSV + "/vulns/" + vid)
            record = records[vid]
            fixed = _fixed_versions(record)
            findings.append({"image": image, "package": name.split("/", 1)[-1], "version": version, "id": vid,
                             "level": osv_level(record), "severity": advisory_severity(record),
                             "fixed": ", ".join(fixed[:3]) or None,
                             "summary": record.get("summary") or record.get("details", "")[:160]})
    return findings


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--sbom", required=True, type=Path, help="the release's sbom directory (with index.json)")
    parser.add_argument("--tracker", type=Path, help="a saved Debian security tracker JSON (fetched otherwise)")
    args = parser.parse_args()
    index = json.loads((args.sbom / "index.json").read_text(encoding="utf-8"))
    if args.tracker:
        tracker = json.loads(args.tracker.read_text(encoding="utf-8"))
    else:
        with tempfile.TemporaryFile() as saved:
            with urllib.request.urlopen(TRACKER, timeout=TIMEOUT * 5) as response:
                saved.write(response.read())
            saved.seek(0)
            tracker = json.load(saved)
    findings, unchecked, records = [], {}, {}
    for image, entry in sorted(index.items()):
        document = json.loads((args.sbom / entry["spdx"]).read_text(encoding="utf-8"))
        findings += osv_findings(image, document, records)
        findings += debian_findings(image, document, tracker)
        unchecked[image] = osv_queries(document)[1]
    result = report(findings, unchecked)
    (args.sbom / "vulns.json").write_text(json.dumps(result, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    (args.sbom / "vulns.md").write_text(markdown(result), encoding="utf-8")
    print(markdown(result).split("\n\n## HIGH")[0])
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
