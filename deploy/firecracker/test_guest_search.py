"""SEC-013 and SEC-019 on the microVM: the in-guest search that is given only hashes of what it looks for (no KVM)."""
import base64
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guest_search as search  # noqa: E402

VALUE = "fake-connector-token-Q2hlY2tlZC1ieS1oYXNoZXM"
FORMS = [("form:raw", VALUE), ("form:base64", base64.b64encode(VALUE.encode()).decode())]


class Tree:
    """A guest-like tree: writable roots, a token file and a /proc with one process's environment."""

    def __init__(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        for name in ("workspace", "run/tqa", "proc/41"):
            (self.root / name).mkdir(parents=True)
        (self.root / "run/tqa/token").write_text("runtime-token-7f1c\n")
        (self.root / "proc/41/environ").write_bytes(b"LANG=C.UTF-8\0TQA_GATEWAY_URL=ws://tqa-gateway:8080/runtime\0")
        (self.root / "proc/41/cmdline").write_bytes(b"tqa-runtime\0")

    def run(self, labelled):
        spec = search.spec(labelled, roots=[str(self.root / "workspace"), str(self.root / "run")],
                           proc=str(self.root / "proc"), token_file=str(self.root / "run/tqa/token"))
        done = subprocess.run([sys.executable, "-c", search.program(spec)], capture_output=True, text=True, timeout=60,
                              check=True)
        return search.report(done.stdout)


class Search(unittest.TestCase):
    def setUp(self):
        self.tree = Tree()
        self.addCleanup(self.tree.directory.cleanup)

    def test_nothing_planted_finds_only_the_two_controls(self):
        found = self.tree.run(FORMS)
        root = self.tree.root
        self.assertEqual(sorted(found["hits"]), sorted([[str(root / "run/tqa/token"), "control:token"],
                                                        [str(root / "proc/41/environ"), "control:gateway"]]))
        self.assertEqual(found["files"], 1)

    def test_every_planted_form_is_found_where_it_is(self):
        root = self.tree.root
        (root / "workspace/notes.txt").write_text("before " + VALUE + " after")
        (root / "workspace/deep").mkdir()
        (root / "workspace/deep/blob.bin").write_bytes(b"\x00" * 5000 + FORMS[1][1].encode() + b"\xff")
        (root / "proc/41/cmdline").write_bytes(b"git\0push\0" + VALUE.encode() + b"\0")
        hits = [tuple(hit) for hit in self.tree.run(FORMS)["hits"]]
        self.assertIn((str(root / "workspace/notes.txt"), "form:raw"), hits)
        self.assertIn((str(root / "workspace/deep/blob.bin"), "form:base64"), hits)
        self.assertIn((str(root / "proc/41/cmdline"), "form:raw"), hits)

    def test_a_near_miss_is_not_a_hit(self):
        root = self.tree.root
        (root / "workspace/near.txt").write_text(VALUE[:-1] + "X " + VALUE[1:] + " " + VALUE[:20])
        self.assertFalse([hit for hit in self.tree.run(FORMS)["hits"] if hit[1].startswith("form:")])

    def test_a_value_at_the_very_start_or_end_of_a_file_is_found(self):
        root = self.tree.root
        (root / "workspace/start.txt").write_text(VALUE + "tail")
        (root / "workspace/end.txt").write_text("head" + VALUE)
        (root / "workspace/exact.txt").write_text(VALUE)
        places = {hit[0] for hit in self.tree.run(FORMS)["hits"] if hit[1] == "form:raw"}
        self.assertEqual(places, {str(root / "workspace" / name) for name in ("start.txt", "end.txt", "exact.txt")})


class Secrecy(unittest.TestCase):
    def test_the_program_holds_no_form_of_the_value_nor_any_long_piece_of_one(self):
        text = search.program(search.spec(FORMS))
        for _, value in FORMS:
            for start in range(len(value) - 7):
                self.assertNotIn(value[start:start + 8], text)

    def test_each_run_has_its_own_salt_and_base(self):
        first, second = search.spec(FORMS), search.spec(FORMS)
        self.assertNotEqual(first["salt"], second["salt"])
        self.assertNotEqual(first["base"], second["base"])
        self.assertNotEqual(first["needles"][0]["digest"], second["needles"][0]["digest"])

    def test_the_default_roots_are_the_guests_writable_trees(self):
        spec = search.spec(FORMS)
        self.assertEqual(spec["roots"], ["/workspace", "/tmp", "/run", "/home", "/var/tmp"])
        self.assertEqual((spec["proc"], spec["token_file"]), ("/proc", "/run/tqa/token"))


class Forms(unittest.TestCase):
    """Every form a credential may be stored or sent in: raw, URL-encoded, base64 whole, and base64 inside anything
    (an HTTP Basic header's user:token among them) at each of the three byte alignments, in both alphabets."""

    def test_the_whole_value_forms_are_kept(self):
        forms = search.forms(VALUE)
        raw = VALUE.encode()
        for form in (VALUE, base64.b64encode(raw).decode(), base64.b64encode(raw).decode().rstrip("="),
                     base64.urlsafe_b64encode(raw).decode()):
            self.assertIn(form, forms)

    def test_an_http_basic_header_is_matched_for_any_user_name(self):
        forms = search.forms(VALUE)
        # User names of every length mod 3, so the token starts at each byte alignment.
        for user in ("x-access-token", "git", "oauth2", "u", "fixture-user", "ab"):
            with self.subTest(user=user):
                for encode in (base64.b64encode, base64.urlsafe_b64encode):
                    header = "Authorization: Basic " + encode((user + ":" + VALUE).encode()).decode()
                    self.assertTrue([form for form in forms if form in header], header)

    def test_base64_of_a_larger_blob_holding_the_value_anywhere_is_matched(self):
        forms = search.forms(VALUE)
        for prefix in range(6):
            with self.subTest(prefix=prefix):
                blob = base64.b64encode(os.urandom(prefix) + VALUE.encode() + os.urandom(5)).decode()
                self.assertTrue([form for form in forms if form in blob])

    def test_no_form_is_short_enough_to_match_by_chance(self):
        self.assertGreaterEqual(min(len(form) for form in search.forms(VALUE)), 32)
        noise = base64.b64encode(os.urandom(1 << 16)).decode()
        self.assertFalse([form for form in search.forms(VALUE) if form in noise])

    def test_a_short_value_is_refused(self):
        with self.assertRaises(ValueError):
            search.forms("short-token")


class HashedControl(unittest.TestCase):
    """A live control only the hash matcher can find: planted by the scripted turn in two halves, so the literal value
    never appears in the command, and given to the search as hashes only."""

    def setUp(self):
        self.tree = Tree()
        self.addCleanup(self.tree.directory.cleanup)

    def test_the_plant_command_never_holds_the_value_whole(self):
        control = search.control_value()
        self.assertNotIn(control, search.plant(control))
        self.assertGreaterEqual(len(control), 32)

    def test_a_planted_control_is_found_by_the_hash_matcher_and_required(self):
        control = search.control_value()
        root = self.tree.root
        planted = root / "workspace/hashed-control"
        plant = search.plant(control, path=str(planted))
        subprocess.run(["sh", "-c", plant], check=True)
        spec = search.spec(FORMS, control=control, roots=[str(root / "workspace"), str(root / "run")],
                           proc=str(root / "proc"), token_file=str(root / "run/tqa/token"))
        self.assertNotIn(control, search.program(spec))
        done = subprocess.run([sys.executable, "-c", search.program(spec)], capture_output=True, text=True,
                              timeout=60, check=True)
        found = search.report(done.stdout)
        self.assertIn([str(planted), "control:hashed"], found["hits"])
        self.assertEqual(search.problems(found), [])

    def test_a_run_without_the_hashed_control_fails(self):
        token = ["/run/tqa/token", "control:token"]
        gateway = ["/proc/9/environ", "control:gateway"]
        self.assertIn("the hash matcher did not find the planted control",
                      search.problems({"hits": [token, gateway]}))


class Report(unittest.TestCase):
    def test_the_report_line_is_found_among_other_output_and_its_absence_fails(self):
        line = search.MARK + json.dumps({"hits": [], "files": 3, "bytes": 10})
        self.assertEqual(search.report("noise\n" + line + "\nmore\n")["files"], 3)
        with self.assertRaises(AssertionError):
            search.report("noise only\n")

    def test_verdict_needs_both_controls_and_names_every_leak(self):
        token = ["/run/tqa/token", "control:token"]
        gateway = ["/proc/9/environ", "control:gateway"]
        hashed = [search.CONTROL_PATH, "control:hashed"]
        self.assertEqual(search.problems({"hits": [token, gateway, hashed]}), [])
        self.assertIn("the search did not read /run/tqa/token", search.problems({"hits": [gateway, hashed]}))
        self.assertIn("the search found no runtime environment in /proc", search.problems({"hits": [token, hashed]}))
        leak = ["/tmp/x", "form:raw"]
        self.assertIn("the credential is in the guest: /tmp/x (form:raw)",
                      search.problems({"hits": [token, gateway, hashed, leak]}))


if __name__ == "__main__":
    os.environ.setdefault("PYTHONDONTWRITEBYTECODE", "1")
    unittest.main()
