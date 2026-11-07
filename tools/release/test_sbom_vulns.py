"""REL-U-003: the release vulnerability gate on fixture SBOMs, OSV records and Debian tracker entries (no network)."""

import unittest

import sbom_vulns as vulns


def spdx(*packages):
    return {"packages": [{"name": "image", "versionInfo": "x", "externalRefs": [
        {"referenceType": "purl", "referenceLocator": "pkg:oci/image"}]}] + list(packages)}


def deb(name, version, source=None, source_version=None):
    return {"name": name, "versionInfo": version,
            "sourceInfo": "built package from: {} {}".format(source or name, source_version or version),
            "externalRefs": [{"referenceType": "purl", "referenceLocator": "pkg:deb/debian/{}@{}?arch=amd64".format(
                name, version)}]}


def purl(name, version, locator):
    return {"name": name, "versionInfo": version, "externalRefs": [
        {"referenceType": "purl", "referenceLocator": locator}]}


def generic(name, version):
    return purl(name, version, "pkg:generic/{}@{}".format(name, version))


TRIXIE = spdx(deb("base-files", "13.7"), deb("libc6", "2.41-12+deb13u4", "glibc"),
              deb("chromium", "153.0.8010.52-1~deb13u1"), deb("linux-libc-dev", "6.12.107-1", "linux"),
              purl("tar", "7.5.19", "pkg:npm/tar@7.5.19"), purl("serde", "1.0.228", "pkg:cargo/serde@1.0.228?x=1"),
              generic("go", "1.27.1"), generic("node", "24.21.0"))


class DebianVersions(unittest.TestCase):
    def test_debian_ordering(self):
        for low, high in (("1.0", "1.1"), ("1.0~rc1", "1.0"), ("2.0", "1:0.9"), ("2.41-12+deb13u4", "2.41-12+deb13u5"),
                          ("153.0.8010.52-1~deb13u1", "154.0.8037.57-1~deb13u1"), ("1.0-1", "1.0-1+b1"),
                          ("1.2.13.dfsg-1", "1:1.2.13.dfsg-1"), ("9.1-1", "9.1-1.1"), ("1.0a", "1.0b")):
            with self.subTest((low, high)):
                self.assertLess(vulns.debian_compare(low, high), 0)
                self.assertGreater(vulns.debian_compare(high, low), 0)
        self.assertEqual(vulns.debian_compare("1:2.5.1-4", "1:2.5.1-4"), 0)


class Severity(unittest.TestCase):
    def test_cvss3_base_scores(self):
        for vector, score in (("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H", 7.5),
                              ("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H", 9.8),
                              ("CVSS:3.1/AV:N/AC:H/PR:N/UI:N/S:U/C:L/I:L/A:N", 4.8),
                              ("CVSS:3.1/AV:N/AC:L/PR:L/UI:N/S:C/C:H/I:H/A:H", 9.9),
                              ("CVSS:3.0/AV:L/AC:L/PR:L/UI:N/S:U/C:N/I:N/A:N", 0.0)):
            with self.subTest(vector):
                self.assertEqual(vulns.cvss3_score(vector), score)
        self.assertIsNone(vulns.cvss3_score("CVSS:4.0/AV:N/AC:L/AT:N/PR:N/UI:N/VC:L/VI:N/VA:N/SC:H/SI:N/SA:N"))

    def test_advisory_severity_prefers_the_reviewed_label(self):
        record = {"database_specific": {"severity": "MODERATE"},
                  "severity": [{"type": "CVSS_V3", "score": "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"}]}
        self.assertEqual(vulns.advisory_severity(record), "MEDIUM")
        del record["database_specific"]
        self.assertEqual(vulns.advisory_severity(record), "CRITICAL")
        self.assertEqual(vulns.advisory_severity({}), "UNKNOWN")


class OsvFindings(unittest.TestCase):
    def record(self, severity, fixed):
        events = [{"introduced": "0"}] + ([{"fixed": fixed}] if fixed else [])
        return {"id": "GHSA-x", "summary": "s", "database_specific": {"severity": severity},
                "affected": [{"ranges": [{"type": "SEMVER", "events": events}]}]}

    def test_a_fixable_high_fails_and_a_fixable_moderate_does_not(self):
        self.assertEqual(vulns.osv_level(self.record("HIGH", "7.5.21")), "HIGH")
        self.assertEqual(vulns.osv_level(self.record("CRITICAL", "1.0")), "HIGH")
        self.assertEqual(vulns.osv_level(self.record("MODERATE", "7.5.21")), "MEDIUM")
        self.assertEqual(vulns.osv_level(self.record("LOW", "7.5.21")), "MEDIUM")

    def test_without_a_fix_it_is_open_whatever_the_severity(self):
        self.assertEqual(vulns.osv_level(self.record("CRITICAL", None)), "OPEN")


class DebianFindings(unittest.TestCase):
    TRACKER = {
        "chromium": {"CVE-1": {"releases": {"trixie": {
            "status": "resolved", "fixed_version": "154.0.8037.57-1~deb13u1", "urgency": "not yet assigned",
            "repositories": {"trixie": "150.0-1~deb13u1", "trixie-security": "154.0.8037.57-1~deb13u1"}}}}},
        "glibc": {
            "CVE-2": {"releases": {"trixie": {"status": "resolved", "fixed_version": "2.41-12+deb13u5",
                                              "urgency": "not yet assigned", "repositories": {"trixie": "2.41-12+deb13u5"}}}},
            "CVE-3": {"releases": {"trixie": {"status": "open", "urgency": "unimportant", "repositories": {}}}},
            "CVE-4": {"releases": {"trixie": {"status": "resolved", "fixed_version": "2.40-1", "urgency": "low",
                                              "repositories": {}}}},
            "CVE-5": {"releases": {"bookworm": {"status": "open", "urgency": "high", "repositories": {}}}}},
        "linux": {"CVE-6": {"releases": {"trixie": {"status": "open", "urgency": "not yet assigned"}}}},
    }

    def findings(self):
        return vulns.debian_findings("img", TRIXIE, self.TRACKER)

    def test_a_pending_security_update_fails_and_a_point_release_fix_does_not(self):
        levels = {(f["package"], f["id"]): f["level"] for f in self.findings()}
        self.assertEqual(levels[("chromium", "CVE-1")], "HIGH")
        self.assertEqual(levels[("glibc", "CVE-2")], "MEDIUM")

    def test_open_issues_are_reported_and_fixed_ones_and_other_releases_are_not(self):
        levels = {(f["package"], f["id"]): f["level"] for f in self.findings()}
        self.assertEqual(levels[("glibc", "CVE-3")], "OPEN")
        self.assertNotIn(("glibc", "CVE-4"), levels)
        self.assertNotIn(("glibc", "CVE-5"), levels)

    def test_kernel_headers_alone_are_not_a_kernel(self):
        self.assertNotIn("linux", {f["package"] for f in self.findings()})

    def test_a_high_urgency_open_issue_is_flagged_but_does_not_fail(self):
        tracker = {"glibc": {"CVE-7": {"releases": {"trixie": {"status": "open", "urgency": "high"}}}}}
        [finding] = vulns.debian_findings("img", TRIXIE, tracker)
        self.assertEqual((finding["level"], finding["severity"]), ("OPEN", "HIGH"))

    def test_the_release_comes_from_base_files_and_an_unknown_one_is_refused(self):
        self.assertEqual(vulns.debian_release(TRIXIE), "trixie")
        self.assertIsNone(vulns.debian_release(spdx(purl("serde", "1", "pkg:cargo/serde@1"))))
        with self.assertRaisesRegex(ValueError, "Debian 99"):
            vulns.debian_release(spdx(deb("base-files", "99.1")))


class Queries(unittest.TestCase):
    def test_each_ecosystem_is_queried_and_uncovered_components_are_listed(self):
        queries, unchecked = vulns.osv_queries(TRIXIE)
        self.assertIn(("pkg:npm/tar@7.5.19", {"package": {"purl": "pkg:npm/tar@7.5.19"}}), queries)
        self.assertIn(("pkg:cargo/serde@1.0.228", {"package": {"purl": "pkg:cargo/serde@1.0.228"}}), queries)
        self.assertIn(("go-stdlib@1.27.1", {"package": {"name": "stdlib", "ecosystem": "Go"}, "version": "1.27.1"}),
                      queries)
        self.assertFalse(any(key.startswith("pkg:deb") for key, _ in queries), "Debian goes to the tracker")
        self.assertEqual(unchecked, ["node 24.21.0"])


class Gate(unittest.TestCase):
    def test_the_gate_fails_on_high_only_and_the_report_names_everything(self):
        findings = [
            {"image": "web", "package": "tar", "version": "7.5.19", "id": "GHSA-x", "level": "HIGH", "severity": "HIGH",
             "fixed": "7.5.21", "summary": "DoS"},
            {"image": "web", "package": "glibc", "version": "2.41", "id": "CVE-3", "level": "OPEN",
             "severity": "UNIMPORTANT", "fixed": None, "summary": ""}]
        report = vulns.report(findings, {"web": ["node 24.21.0"]})
        self.assertFalse(report["passed"])
        self.assertEqual(report["counts"]["web"], {"HIGH": 1, "OPEN": 1})
        text = vulns.markdown(report)
        self.assertIn("| web | tar | 7.5.19 | GHSA-x | HIGH | 7.5.21 |", text)
        self.assertIn("node 24.21.0", text)
        self.assertTrue(vulns.report(findings[1:], {})["passed"])


if __name__ == "__main__":
    unittest.main()
