"""The wave stack profile (E2E-WAVE-002) without a stack: what it would change, and a round trip against a fake of the
admin limits API. Run: python3 -m unittest tests/e2e/test_wave_profile.py"""

import contextlib
import io
import json
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import wave_profile  # noqa: E402

TOKEN = "admin-fixture-marker"


def listing(slots, per_user, max_live=8):
    return [
        {"key": "admission.slots", "value": slots, "min": 1, "max": max_live, "version": 1},
        {"key": "admission.per_user", "value": per_user, "min": 1, "max": max_live, "version": 1},
        {"key": "sandbox.cpu_millicores", "value": 1000, "min": 100, "max": 4000, "version": 1},
    ]


class Plan(unittest.TestCase):
    def test_raises_slots_before_per_user(self):
        self.assertEqual(wave_profile.plan(listing(4, 4), 8),
                         ([("admission.slots", 8), ("admission.per_user", 8)], None))

    def test_changes_nothing_when_the_stack_already_admits_the_wave(self):
        self.assertEqual(wave_profile.plan(listing(8, 8), 8), ([], None))
        self.assertEqual(wave_profile.plan(listing(12, 10), 8), ([], None))

    def test_raises_only_what_is_short(self):
        self.assertEqual(wave_profile.plan(listing(8, 2), 8), ([("admission.per_user", 8)], None))

    def test_refuses_when_the_file_caps_sandboxes_below_the_wave(self):
        writes, problem = wave_profile.plan(listing(4, 4, max_live=4), 8)
        self.assertEqual(writes, [])
        self.assertIn("sandbox.max_live", problem)
        self.assertIn("8", problem)

    def test_refuses_a_listing_without_the_admission_keys(self):
        writes, problem = wave_profile.plan(listing(4, 4)[2:], 8)
        self.assertEqual(writes, [])
        self.assertIn("admission.slots", problem)

    def test_restore_lowers_per_user_before_slots(self):
        before = {"admission.slots": 4, "admission.per_user": 2}
        self.assertEqual(wave_profile.restore_plan(before, listing(8, 8)),
                         [("admission.per_user", 2), ("admission.slots", 4)])
        self.assertEqual(wave_profile.restore_plan(before, listing(4, 2)), [])


class FakeGateway:
    """The admin limits API as the gateway answers it: versions, the order rule (per_user at most slots), the bearer
    token and an idempotency key on every write."""

    def __init__(self, slots, per_user, max_live=8):
        self.items = {item["key"]: item for item in listing(slots, per_user, max_live)}
        self.writes = []
        fake = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def answer(self, status, body):
                data = json.dumps(body).encode()
                self.send_response(status)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def authorised(self):
                if self.headers.get("authorization") != "Bearer " + TOKEN:
                    self.answer(401, {"error": {"code": "unauthorized"}})
                    return False
                return True

            def do_GET(self):
                if not self.authorised():
                    return
                if self.path != "/api/v1/admin/limits":
                    return self.answer(404, {"error": {"code": "not_found"}})
                self.answer(200, {"items": list(fake.items.values()), "generation": 1})

            def do_PUT(self):
                if not self.authorised():
                    return
                key = self.path.rsplit("/", 1)[-1]
                body = json.loads(self.rfile.read(int(self.headers["content-length"])))
                item = fake.items.get(key)
                if item is None:
                    return self.answer(404, {"error": {"code": "not_found"}})
                if not self.headers.get("idempotency-key"):
                    return self.answer(400, {"error": {"code": "idempotency_key_required"}})
                if body["version"] != item["version"]:
                    return self.answer(409, {"error": {"code": "version_conflict"}})
                value = body["value"]
                slots = value if key == "admission.slots" else fake.items["admission.slots"]["value"]
                per_user = value if key == "admission.per_user" else fake.items["admission.per_user"]["value"]
                if not item["min"] <= value <= item["max"] or per_user > slots:
                    return self.answer(422, {"error": {"code": "limit_out_of_range", "field": key}})
                item["value"], item["version"] = value, item["version"] + 1
                fake.writes.append((key, value))
                self.answer(200, item)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.base_url = "http://127.0.0.1:{}".format(self.server.server_address[1])
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def values(self):
        return {key: self.items[key]["value"] for key in ("admission.slots", "admission.per_user")}

    def close(self):
        self.server.shutdown()
        self.server.server_close()


class RoundTrip(unittest.TestCase):
    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        self.token_file = self.directory / "token"
        self.token_file.write_text(TOKEN + "\n")
        self.token_file.chmod(0o600)

    def run_cli(self, *args):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
            code = wave_profile.main(list(args))
        return code, out.getvalue()

    def test_apply_records_then_restore_returns_to_the_recorded_values(self):
        gateway = FakeGateway(4, 2)
        self.addCleanup(gateway.close)
        record = self.directory / "run"
        common = ["--base-url", gateway.base_url, "--token-file", str(self.token_file)]
        code, out = self.run_cli("apply", *common, "--concurrency", "8", "--record", str(record))
        self.assertEqual(code, 0, out)
        self.assertEqual(gateway.values(), {"admission.slots": 8, "admission.per_user": 8})
        before = json.loads((record / "wave-profile-before.json").read_text())
        self.assertEqual(before["limits"], {"admission.slots": 4, "admission.per_user": 2})
        self.assertEqual(before["concurrency"], 8)
        # A second apply never overwrites the record it would restore from.
        code, out = self.run_cli("apply", *common, "--concurrency", "8", "--record", str(record))
        self.assertNotEqual(code, 0)
        self.assertIn("wave-profile-before.json", out)
        code, out = self.run_cli("restore", *common, "--record", str(record))
        self.assertEqual(code, 0, out)
        self.assertEqual(gateway.values(), {"admission.slots": 4, "admission.per_user": 2})
        self.assertEqual(gateway.writes, [("admission.slots", 8), ("admission.per_user", 8),
                                          ("admission.per_user", 2), ("admission.slots", 4)])
        self.assertNotIn(TOKEN, out)

    def test_check_reports_without_writing(self):
        gateway = FakeGateway(4, 4, max_live=4)
        self.addCleanup(gateway.close)
        code, out = self.run_cli("check", "--base-url", gateway.base_url, "--token-file", str(self.token_file),
                                 "--concurrency", "8")
        self.assertNotEqual(code, 0)
        self.assertIn("sandbox.max_live", out)
        self.assertEqual(gateway.writes, [])
        gateway = FakeGateway(8, 8)
        self.addCleanup(gateway.close)
        code, out = self.run_cli("check", "--base-url", gateway.base_url, "--token-file", str(self.token_file),
                                 "--concurrency", "8")
        self.assertEqual(code, 0, out)
        self.assertEqual(gateway.writes, [])

    def test_refuses_a_token_file_others_can_read(self):
        self.token_file.chmod(0o644)
        code, out = self.run_cli("check", "--base-url", "http://127.0.0.1:9", "--token-file", str(self.token_file),
                                 "--concurrency", "8")
        self.assertNotEqual(code, 0)
        self.assertIn("0600", out)
        self.assertNotIn(TOKEN, out)


if __name__ == "__main__":
    unittest.main()
