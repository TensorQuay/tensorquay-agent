"""An artefact's SPDX 2.3 JSON document and its readable package list (ADR-0017 point 7), from `sbom_sources` records.

The output is deterministic: the same packages and creation time give byte-identical bytes, so a rebuilt image can be
compared with its release.
"""

from dataclasses import dataclass
import datetime
import json
import re
from urllib.parse import quote

TOOL = "Tool: tqa-sbom-1"
NAMESPACE = "https://tensorquay.com/spdx"


@dataclass(frozen=True)
class Subject:
    """The artefact a document describes: an image (or a rootfs, a kernel) by its SHA-256 digest."""
    name: str
    version: str
    digest: str  # "sha256:<hex>"
    kind: str = "image"  # "image", "rootfs" or "kernel"

    @property
    def hex(self):
        algorithm, _, value = self.digest.partition(":")
        if algorithm != "sha256" or not re.fullmatch(r"[0-9a-f]{64}", value):
            raise ValueError("the subject needs a sha256 digest: " + self.digest)
        return value


PURPOSE = {"image": "CONTAINER", "rootfs": "FILE", "kernel": "OPERATING-SYSTEM"}


def _subject_purl(subject):
    """An image by its OCI digest; a rootfs or kernel file by name, version and checksum."""
    if subject.kind == "image":
        return "pkg:oci/{}@sha256%3A{}".format(subject.name, subject.hex)
    return "pkg:generic/{}@{}?checksum=sha256:{}".format(subject.name, quote(subject.version), subject.hex)


def _spdx_id(prefix, *parts):
    """An SPDX identifier: letters, digits, dots and hyphens only."""
    return "SPDXRef-" + "-".join(re.sub(r"[^A-Za-z0-9.]+", "-", part).strip("-") for part in (prefix, *parts))


def _timestamp(created):
    return datetime.datetime.fromtimestamp(created, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _package(package, spdx_id):
    entry = {
        "SPDXID": spdx_id,
        "name": package.name,
        "versionInfo": package.version,
        "downloadLocation": package.download_location or "NOASSERTION",
        "filesAnalyzed": False,
        "licenseConcluded": "NOASSERTION",
        "licenseDeclared": package.license,
        "copyrightText": "NOASSERTION",
        "externalRefs": [{"referenceCategory": "PACKAGE-MANAGER", "referenceType": "purl",
                          "referenceLocator": package.purl}],
    }
    if package.sha256:
        entry["checksums"] = [{"algorithm": "SHA256", "checksumValue": package.sha256}]
    if package.kind == "deb":
        entry["sourceInfo"] = "built package from: {} {}".format(package.source_name, package.source_version)
    return entry


def _extracted(package):
    """What a `LicenseRef` stands for: the package's own notice, in the image and at its matching source."""
    text = "The licence terms are those of the notice at {} in the image".format(package.license_notice)
    if package.kind == "deb":
        text += ", also at https://sources.debian.org/src/{}/{}/debian/copyright/".format(
            quote(package.source_name), quote(package.source_version))
    return {"licenseId": package.license, "name": package.license.removeprefix("LicenseRef-"),
            "extractedText": text + "."}


def document(subject, packages, created):
    """The SPDX 2.3 document: the artefact first, `DESCRIBES`d by the document, then every package it `CONTAINS`."""
    image_id = _spdx_id("image", subject.name, subject.version)
    image = {
        "SPDXID": image_id,
        "name": subject.name,
        "versionInfo": subject.version,
        "downloadLocation": "NOASSERTION",
        "filesAnalyzed": False,
        "licenseConcluded": "NOASSERTION",
        "licenseDeclared": "NOASSERTION",
        "copyrightText": "NOASSERTION",
        "primaryPackagePurpose": PURPOSE[subject.kind],
        "checksums": [{"algorithm": "SHA256", "checksumValue": subject.hex}],
        "externalRefs": [{"referenceCategory": "PACKAGE-MANAGER", "referenceType": "purl",
                          "referenceLocator": _subject_purl(subject)}],
    }
    entries, relationships, extracted, seen = [], [], {}, set()
    ordered = sorted(packages, key=lambda item: (item.kind, item.name, item.version, item.architecture))
    for package in ordered:
        spdx_id = _spdx_id(package.kind, package.name, package.version, package.architecture)
        suffix = 1
        while spdx_id in seen:
            suffix += 1
            spdx_id = _spdx_id(package.kind, package.name, package.version, package.architecture, str(suffix))
        seen.add(spdx_id)
        entries.append(_package(package, spdx_id))
        relationships.append({"spdxElementId": image_id, "relationshipType": "CONTAINS",
                              "relatedSpdxElement": spdx_id})
        if package.license.startswith("LicenseRef-"):
            extracted[package.license] = _extracted(package)
    result = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": "{}-{}".format(subject.name, subject.version),
        "documentNamespace": "{}/{}/{}".format(NAMESPACE, subject.name, subject.hex),
        "creationInfo": {"created": _timestamp(created), "creators": ["Organization: TensorQuay", TOOL]},
        "packages": [image] + entries,
        "relationships": [{"spdxElementId": "SPDXRef-DOCUMENT", "relationshipType": "DESCRIBES",
                           "relatedSpdxElement": image_id}] + relationships,
    }
    if extracted:
        result["hasExtractedLicensingInfos"] = [extracted[key] for key in sorted(extracted)]
    return result


def dumps(value):
    """The document's bytes: sorted keys, two-space indent, a final newline."""
    return json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def _cell(text):
    return str(text).replace("|", "&#124;").replace("\n", " ")


def package_list(value):
    """The readable list for the release notes: every package with its exact version, licence and matching source."""
    image = value["packages"][0]
    lines = [
        "# {} {} packages".format(image["name"], image["versionInfo"]), "",
        "Digest: `sha256:{}`. Generated from the SPDX document; do not edit by hand.".format(
            image["checksums"][0]["checksumValue"]), "",
        "| Package | Version | Licence | Matching source |", "| --- | --- | --- | --- |",
    ]
    for package in value["packages"][1:]:
        lines.append("| {} | {} | {} | [source]({}) |".format(
            _cell(package["name"]), _cell(package["versionInfo"]), _cell(package["licenseDeclared"]),
            package["downloadLocation"]))
    return "\n".join(lines) + "\n"
