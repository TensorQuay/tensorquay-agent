"""H6: the end-of-run orphan audit, on synthetic listings (no Docker)."""
import unittest

from soak_orphans import findings, parse_rows

CONVERSATIONS = {"a", "b", "c"}


def container(identity, conversation, state="running"):
    return {"id": identity, "conversation": conversation, "state": state, "networks": ["tqa-sbx-" + identity]}


class Findings(unittest.TestCase):
    def test_a_clean_end_has_no_findings(self):
        containers = [container("1", "a"), container("2", "b")]
        rows = [{"conversation": "a", "phase": "live"}, {"conversation": "b", "phase": "live"}]
        networks = [{"name": "tqa-sbx-1", "attached": 1}, {"name": "tqa-sbx-2", "attached": 1}]
        self.assertEqual(findings(containers, rows, networks, [], CONVERSATIONS), [])

    def test_every_kind_of_leftover_is_named(self):
        containers = [container("1", "a", state="exited"), container("2", "zzz"), container("3", "c")]
        rows = [{"conversation": "a", "phase": "live"}, {"conversation": "b", "phase": "destroying"}]
        networks = [{"name": "tqa-sbx-1", "attached": 1}, {"name": "tqa-sbx-9", "attached": 0}]
        volumes = [{"name": "tqa-leftover", "in_use": False}]
        problems = findings(containers, rows, networks, volumes, CONVERSATIONS)
        joined = "\n".join(problems)
        for expected in ("container 1 (conversation a) is exited", "container 2 belongs to no conversation of this soak",
                         "conversation c has a running sandbox but no admission row",
                         "admission row for conversation b is destroying",
                         "conversation b has an admission row but no running sandbox",
                         "network tqa-sbx-9 has no container", "volume tqa-leftover is unused"):
            self.assertIn(expected, joined)

    def test_two_sandboxes_for_one_conversation_is_a_leak(self):
        containers = [container("1", "a"), container("2", "a")]
        rows = [{"conversation": "a", "phase": "live"}]
        networks = [{"name": "tqa-sbx-1", "attached": 1}, {"name": "tqa-sbx-2", "attached": 1}]
        self.assertIn("conversation a has 2 running sandboxes", "\n".join(
            findings(containers, rows, networks, [], CONVERSATIONS)))


class Rows(unittest.TestCase):
    def test_psql_rows_parse(self):
        self.assertEqual(parse_rows("a|live\nb|starting\n\n"), [{"conversation": "a", "phase": "live"},
                                                               {"conversation": "b", "phase": "starting"}])


if __name__ == "__main__":
    unittest.main()
