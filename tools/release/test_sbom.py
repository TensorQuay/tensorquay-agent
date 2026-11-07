"""REL-U-001: the SBOM generator, on fixture image trees (no Docker, no network).

Run: python3 -m unittest discover -s tools/release -p 'test_*.py' -v
"""

import json
from pathlib import Path
import tempfile
import textwrap
import unittest

import sbom_sources as sources
import sbom_spdx as spdx

DEP5 = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n"

STATUS = textwrap.dedent("""\
    Package: bash
    Status: install ok installed
    Architecture: amd64
    Version: 5.2.37-2+b5

    Package: libc6
    Status: install ok installed
    Architecture: amd64
    Source: glibc (2.41-12)
    Version: 2.41-12

    Package: libgcc-s1
    Status: install ok installed
    Architecture: amd64
    Source: gcc-14
    Version: 14.2.0-19

    Package: removed-tool
    Status: deinstall ok config-files
    Architecture: amd64
    Version: 1.0-1

    Package: weird
    Status: install ok installed
    Architecture: all
    Version: 0.1-1
    """)


def write(root, relative, text):
    path = Path(root) / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def image_tree(root):
    """A small Debian image: dpkg's status, machine-readable and legacy copyright files, and shipped npm packages."""
    write(root, "var/lib/dpkg/status", STATUS)
    write(root, "usr/share/doc/bash/copyright", DEP5 + "\nFiles: *\nLicense: GPL-3+\n")
    write(root, "usr/share/doc/libc6/copyright", "This is the Debian prepackaged version of the GNU C Library.\n")
    write(root, "usr/share/doc/libgcc-s1/copyright",
          DEP5 + "\nFiles: *\nLicense: GPL-3+ with GCC-exception-3.1\n\nFiles: libiberty/*\nLicense: LGPL-2.1+\n")
    write(root, "usr/share/doc/weird/copyright", DEP5 + "\nFiles: *\nLicense: made-up-terms\n")
    modules = "usr/lib/node_modules"
    write(root, modules + "/lodash/package.json", json.dumps({"name": "lodash", "version": "4.17.21", "license": "MIT"}))
    write(root, modules + "/lodash/dist/package.json", json.dumps({"type": "module"}))
    write(root, modules + "/@scope/pkg/package.json",
          json.dumps({"name": "@scope/pkg", "version": "1.0.0", "license": "Apache-2.0"}))
    write(root, modules + "/@scope/pkg/node_modules/inner/package.json",
          json.dumps({"name": "inner", "version": "2.0.0", "licenses": [{"type": "ISC"}]}))


def cargo_fixture():
    metadata = {"packages": [
        {"name": "tqa-gateway", "version": "0.1.0", "license": "Apache-2.0", "source": None,
         "id": "path+file:///work/crates/tqa-gateway#0.1.0"},
        {"name": "serde", "version": "1.0.219", "license": "MIT OR Apache-2.0",
         "source": "registry+https://github.com/rust-lang/crates.io-index", "id": "serde"},
        {"name": "proptest", "version": "1.11.0", "license": "MIT OR Apache-2.0",
         "source": "registry+https://github.com/rust-lang/crates.io-index", "id": "proptest"},
    ]}
    # `cargo tree -e normal --prefix none` for the shipped binary: the dev-only proptest is not in it.
    tree = "tqa-gateway v0.1.0 (/work/crates/tqa-gateway)\nserde v1.0.219\nserde v1.0.219 (*)\n"
    lock = textwrap.dedent("""\
        [[package]]
        name = "serde"
        version = "1.0.219"
        source = "registry+https://github.com/rust-lang/crates.io-index"
        checksum = "5f0e2c6ed6606019b4e29e69dbaba95b11854410e5347d525002456dbbb786b6"

        [[package]]
        name = "proptest"
        version = "1.11.0"
        source = "registry+https://github.com/rust-lang/crates.io-index"
        checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        """)
    return metadata, tree, lock


class DebianPackages(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        image_tree(self.root)

    def test_installed_packages_with_exact_versions_and_their_source_packages(self):
        packages = {package.name: package for package in sources.debian_packages(self.root)}
        self.assertEqual(sorted(packages), ["bash", "libc6", "libgcc-s1", "weird"], "the removed package is not listed")
        libc = packages["libc6"]
        self.assertEqual((libc.version, libc.source_name, libc.source_version), ("2.41-12", "glibc", "2.41-12"))
        self.assertEqual(libc.download_location, "https://snapshot.debian.org/package/glibc/2.41-12/")
        self.assertEqual(libc.purl, "pkg:deb/debian/libc6@2.41-12?arch=amd64")
        # A binary built from a source of the same name, and one whose source keeps the binary's version.
        self.assertEqual((packages["bash"].source_name, packages["bash"].source_version), ("bash", "5.2.37-2+b5"))
        self.assertEqual((packages["libgcc-s1"].source_name, packages["libgcc-s1"].source_version),
                         ("gcc-14", "14.2.0-19"))

    def test_licences_are_exact_spdx_ids_or_a_reference_never_a_guess(self):
        packages = {package.name: package for package in sources.debian_packages(self.root)}
        self.assertEqual(packages["bash"].license, "GPL-3.0-or-later")
        # Legacy prose, an exception and an unknown name all stay references to the package's own notice.
        for name in ("libc6", "libgcc-s1", "weird"):
            self.assertEqual(packages[name].license, "LicenseRef-debian-" + name, name)
            self.assertEqual(packages[name].license_notice, "/usr/share/doc/{}/copyright".format(name))

    def test_a_reference_is_a_valid_spdx_id_for_any_package_name(self):
        # SPDX ids allow letters, digits, "." and "-" only; Debian names may hold "+".
        self.assertEqual(sources.debian_license("libstdc++6", ""), "LicenseRef-debian-libstdcplusplus6")
        self.assertEqual(sources.debian_license("g++", ""), "LicenseRef-debian-gplusplus")

    def test_several_licences_join_with_and(self):
        write(self.root, "usr/share/doc/bash/copyright",
              DEP5 + "\nFiles: *\nLicense: GPL-3+\n\nFiles: lib/*\nLicense: BSD-3-clause or Expat\n")
        bash = next(package for package in sources.debian_packages(self.root) if package.name == "bash")
        self.assertEqual(bash.license, "(BSD-3-Clause OR MIT) AND GPL-3.0-or-later")

    def test_spdx_spellings_and_debian_names_of_spdx_licences_map_exactly(self):
        write(self.root, "usr/share/doc/bash/copyright",
              DEP5 + "\nFiles: *\nLicense: OpenLDAP-2.8\n\nFiles: a/*\nLicense: GPL-2.0+\n\nFiles: b/*\nLicense: curl\n")
        bash = next(package for package in sources.debian_packages(self.root) if package.name == "bash")
        self.assertEqual(bash.license, "GPL-2.0-or-later AND OLDAP-2.8 AND curl")
        # Perl's "Artistic" names no version, so a package using it keeps its reference.
        write(self.root, "usr/share/doc/bash/copyright", DEP5 + "\nFiles: *\nLicense: GPL-1+ or Artistic\n")
        bash = next(package for package in sources.debian_packages(self.root) if package.name == "bash")
        self.assertEqual(bash.license, "LicenseRef-debian-bash")

    def test_a_missing_notice_is_an_error(self):
        (self.root / "usr/share/doc/weird/copyright").unlink()
        with self.assertRaisesRegex(ValueError, "weird"):
            sources.debian_packages(self.root)


class NpmPackages(unittest.TestCase):
    def test_only_package_roots_are_listed_with_their_registry_tarballs(self):
        with tempfile.TemporaryDirectory() as directory:
            image_tree(directory)
            packages = sources.npm_packages(Path(directory), ["usr/lib/node_modules"])
        self.assertEqual(
            [(package.name, package.version, package.license, package.download_location) for package in packages],
            [("@scope/pkg", "1.0.0", "Apache-2.0", "https://registry.npmjs.org/@scope/pkg/-/pkg-1.0.0.tgz"),
             ("inner", "2.0.0", "ISC", "https://registry.npmjs.org/inner/-/inner-2.0.0.tgz"),
             ("lodash", "4.17.21", "MIT", "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz")])
        self.assertEqual(packages[0].purl, "pkg:npm/%40scope/pkg@1.0.0")

    def test_a_package_without_a_licence_is_an_error(self):
        with tempfile.TemporaryDirectory() as directory:
            write(directory, "node_modules/bare/package.json", json.dumps({"name": "bare", "version": "1.0.0"}))
            with self.assertRaisesRegex(ValueError, "bare"):
                sources.npm_packages(Path(directory), ["node_modules"])


class CargoPackages(unittest.TestCase):
    def test_the_shipped_binarys_closure_with_checksums_and_no_dev_only_crate(self):
        metadata, tree, lock = cargo_fixture()
        packages = sources.cargo_packages(metadata, tree, lock)
        self.assertEqual([(package.name, package.version) for package in packages],
                         [("serde", "1.0.219"), ("tqa-gateway", "0.1.0")])
        serde, own = packages
        self.assertEqual(serde.download_location, "https://static.crates.io/crates/serde/serde-1.0.219.crate")
        self.assertEqual(serde.sha256, "5f0e2c6ed6606019b4e29e69dbaba95b11854410e5347d525002456dbbb786b6")
        self.assertEqual((serde.license, serde.purl), ("MIT OR Apache-2.0", "pkg:cargo/serde@1.0.219"))
        # The project's own crate: its licence, and the project's source rather than a registry.
        self.assertEqual((own.license, own.download_location),
                         ("Apache-2.0", "https://github.com/TensorQuay/tensorquay-agent"))

    def test_the_legacy_slash_separator_is_cargos_or(self):
        metadata, tree, lock = cargo_fixture()
        metadata["packages"][1]["license"] = "MIT/Apache-2.0"
        serde = sources.cargo_packages(metadata, tree, lock)[0]
        self.assertEqual(serde.license, "MIT OR Apache-2.0")
        metadata["packages"][1]["license"] = "Apache-2.0 / MIT"
        self.assertEqual(sources.cargo_packages(metadata, tree, lock)[0].license, "Apache-2.0 OR MIT")

    def test_a_crate_outside_the_metadata_is_an_error(self):
        metadata, _, lock = cargo_fixture()
        with self.assertRaisesRegex(ValueError, "unknown-crate"):
            sources.cargo_packages(metadata, "unknown-crate v1.0.0\n", lock)


class Document(unittest.TestCase):
    def build(self):
        with tempfile.TemporaryDirectory() as directory:
            image_tree(directory)
            packages = sources.debian_packages(Path(directory))
            packages += sources.npm_packages(Path(directory), ["usr/lib/node_modules"])
        metadata, tree, lock = cargo_fixture()
        packages += sources.cargo_packages(metadata, tree, lock)
        packages.append(sources.Package(
            name="linux", version="6.18.54", kind="generic", license="GPL-2.0-only",
            download_location="https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.18.54.tar.xz",
            sha256="9df30b02dd8102bbd0be52556288ef6889ddbe7f1ddb96fbf847d0becf3eacac"))
        subject = spdx.Subject(name="tqa-runtime", version="base", digest="sha256:" + "ab" * 32)
        return spdx.document(subject, packages, created=1790000000), packages

    def test_an_spdx_2_3_document_describing_the_image_that_contains_every_package(self):
        document, packages = self.build()
        self.assertEqual((document["spdxVersion"], document["dataLicense"], document["SPDXID"]),
                         ("SPDX-2.3", "CC0-1.0", "SPDXRef-DOCUMENT"))
        self.assertEqual(document["creationInfo"]["created"], "2026-09-21T14:13:20Z")
        ids = [package["SPDXID"] for package in document["packages"]]
        self.assertEqual(len(ids), len(set(ids)), "unique SPDX ids")
        image = document["packages"][0]
        self.assertEqual((image["name"], image["primaryPackagePurpose"]), ("tqa-runtime", "CONTAINER"))
        self.assertEqual(image["checksums"], [{"algorithm": "SHA256", "checksumValue": "ab" * 32}])
        relationships = {(item["spdxElementId"], item["relationshipType"], item["relatedSpdxElement"])
                         for item in document["relationships"]}
        self.assertIn(("SPDXRef-DOCUMENT", "DESCRIBES", image["SPDXID"]), relationships)
        contained = {item[2] for item in relationships if item[1] == "CONTAINS" and item[0] == image["SPDXID"]}
        self.assertEqual(contained, set(ids[1:]))
        self.assertEqual(len(ids), len(packages) + 1)

    def test_every_listed_package_carries_its_version_source_licence_and_purl(self):
        document, _ = self.build()
        for package in document["packages"][1:]:
            self.assertTrue(package["versionInfo"] and package["downloadLocation"].startswith("https://"), package)
            self.assertFalse(package["filesAnalyzed"])
            refs = [ref["referenceType"] for ref in package.get("externalRefs", [])]
            self.assertIn("purl", refs, package["name"])
        by_name = {package["name"]: package for package in document["packages"]}
        self.assertEqual(by_name["libc6"]["sourceInfo"], "built package from: glibc 2.41-12")
        self.assertEqual(by_name["linux"]["checksums"][0]["checksumValue"], "9df30b02dd8102bbd0be52556288ef6889ddbe7f1ddb96fbf847d0becf3eacac")

    def test_a_rootfs_or_kernel_is_named_by_a_generic_purl_not_an_oci_image(self):
        for kind, purpose in (("rootfs", "FILE"), ("kernel", "OPERATING-SYSTEM")):
            with self.subTest(kind):
                subject = spdx.Subject(name="tqa-" + kind, version="base", digest="sha256:" + "ab" * 32, kind=kind)
                artefact = spdx.document(subject, [], created=0)["packages"][0]
                self.assertEqual(artefact["primaryPackagePurpose"], purpose)
                self.assertEqual(artefact["externalRefs"][0]["referenceLocator"],
                                 "pkg:generic/tqa-{}@base?checksum=sha256:{}".format(kind, "ab" * 32))

    def test_each_licence_reference_is_explained_by_its_notice(self):
        document, _ = self.build()
        used = {package["licenseDeclared"] for package in document["packages"]
                if package["licenseDeclared"].startswith("LicenseRef-")}
        explained = {item["licenseId"]: item for item in document["hasExtractedLicensingInfos"]}
        self.assertEqual(set(explained), used)
        self.assertIn("/usr/share/doc/libc6/copyright", explained["LicenseRef-debian-libc6"]["extractedText"])

    def test_the_same_input_gives_byte_identical_output(self):
        first, _ = self.build()
        second, _ = self.build()
        self.assertEqual(spdx.dumps(first), spdx.dumps(second))

    def test_the_readable_list_has_one_row_per_package_with_its_source(self):
        document, packages = self.build()
        text = spdx.package_list(document)
        for package in packages:
            self.assertIn("| {} | {} |".format(package.name, package.version), text)
        self.assertIn("(https://snapshot.debian.org/package/glibc/2.41-12/)", text)


if __name__ == "__main__":
    unittest.main()
