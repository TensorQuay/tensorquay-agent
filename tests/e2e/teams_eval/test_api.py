"""The agent-teams evaluation's API calls without a gateway: a fake opener answers them, so the paths, the write keys,
the limits' versions and the paging are checked offline.

Run: python3 -m unittest tests/e2e/teams_eval/test_api.py"""

import io
import json
import os
import sys
import tempfile
import unittest
import urllib.error
import urllib.parse
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from api import Api, ApiError  # noqa: E402


class Response(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
        return False


class Opener:
    """Answers each request with the next of `answers` (a JSON value, or an HTTP status to fail with)."""

    def __init__(self, *answers):
        self.answers = list(answers)
        self.requests = []

    def __call__(self, request, timeout):
        url = urllib.parse.urlsplit(request.full_url)
        body = json.loads(request.data) if request.data else None
        self.requests.append((request.get_method(), url.path, dict(urllib.parse.parse_qsl(url.query)), body,
                              request.headers))
        answer = self.answers.pop(0)
        if isinstance(answer, tuple):  # (status, code)
            raise urllib.error.HTTPError(request.full_url, answer[0], "x", {},
                                         io.BytesIO(json.dumps({"error": {"code": answer[1]}}).encode()))
        if isinstance(answer, int):
            raise urllib.error.HTTPError(request.full_url, answer, "x", {},
                                         io.BytesIO(b'{"error": {"code": "not_found"}}'))
        return Response(json.dumps(answer).encode() if answer is not None else b"")


def api(*answers):
    with tempfile.NamedTemporaryFile("w", delete=False) as handle:
        handle.write("secret\n")
    opener = Opener(*answers)
    client = Api("http://gw:8090/", handle.name, opener)
    os.unlink(handle.name)  # the token is read once, at construction
    return client, opener


class Calls(unittest.TestCase):
    def test_writes_carry_a_fresh_key_and_reads_none(self):
        client, opener = api({"id": "w1"}, {"id": "w2"}, {"items": [], "next_after": None})
        client.create_workspace("a")
        client.create_workspace("b")
        client.events("c1")
        keys = [headers.get("Idempotency-key") for _, _, _, _, headers in opener.requests]
        self.assertTrue(keys[0] and keys[1] and keys[0] != keys[1])
        self.assertIsNone(keys[2])
        self.assertEqual(opener.requests[0][4]["Authorization"], "Bearer secret")

    def test_the_team_limits_are_written_with_the_versions_read(self):
        listed = {"items": [{"key": "agent_teams.token_budget", "version": 4},
                            {"key": "agent_teams.deadline_minutes", "version": 9}]}
        client, opener = api(listed, None, None)
        client.set_team_limits(800000, 26)
        self.assertEqual([(method, path, body) for method, path, _, body, _ in opener.requests[1:]],
                         [("PUT", "/api/v1/admin/limits/agent_teams.token_budget", {"value": 800000, "version": 4}),
                          ("PUT", "/api/v1/admin/limits/agent_teams.deadline_minutes", {"value": 26, "version": 9})])

    def test_limits_missing_while_agent_teams_are_off_are_refused(self):
        client, _ = api({"items": []})
        with self.assertRaisesRegex(ApiError, "agent teams off"):
            client.set_team_limits(1, 1)

    def test_pages_are_followed_to_their_end(self):
        client, opener = api({"items": [{"seq": 1}], "next_after": 1}, {"items": [{"seq": 2}], "next_after": None},
                             {"items": [{"seq": 1}], "last_seq": 1, "more": True},
                             {"items": [{"seq": 2}], "last_seq": 2, "more": False},
                             {"items": [{"name": "a"}], "next_after": "a"}, {"items": [{"name": "b"}]})
        self.assertEqual(len(client.events("c1")), 2)
        self.assertEqual(len(client.team_messages("c1")), 2)
        self.assertEqual([item["name"] for item in client.files("c1", "src")], ["a", "b"])
        self.assertEqual(opener.requests[1][2]["after"], "1")
        self.assertEqual(opener.requests[3][2]["after"], "1")
        self.assertEqual(opener.requests[5][2], {"path": "src", "after": "a"})

    def test_a_task_without_a_team_reads_as_none(self):
        client, _ = api(404)
        self.assertIsNone(client.team("c1"))

    def test_a_single_side_task_reads_as_no_team_with_agent_teams_off(self):
        # The single side runs with [agent_teams] off: the team route answers 403 teams_disabled, which is "no team",
        # not a harness problem (found by the kill-and-resume test, 7 Oct 2026: every single attempt went unchecked).
        client, _ = api((403, "teams_disabled"))
        self.assertIsNone(client.team("c1"))
        client, _ = api((403, "forbidden"))
        with self.assertRaises(ApiError):
            client.team("c1")

    def test_a_refusal_names_its_step_status_and_code(self):
        client, _ = api(404)
        with self.assertRaises(ApiError) as caught:
            client.delete_conversation("c1")
        self.assertEqual((caught.exception.step, caught.exception.status, caught.exception.code),
                         ("delete the task", 404, "not_found"))


if __name__ == "__main__":
    unittest.main()
