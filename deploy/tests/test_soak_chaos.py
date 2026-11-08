"""SOAK-002: reproducible fault coverage and bounded namespace helpers."""
import unittest

from soak_audit import AuditError
from soak_chaos import (DATABASE_QUERY, STALE_SECONDS, database_sample, observer_arguments, process_sample,
                        restart_times, runtime_identity, schedule)


class ChaosTests(unittest.TestCase):
    def test_seed_reproduces_bounded_intervals_and_covers_all_targets(self):
        faults = schedule(731, 7200)
        self.assertEqual(faults, schedule(731, 7200))
        self.assertNotEqual(faults, schedule(732, 7200))
        previous = 0
        seen = set()
        for fault in faults:
            if fault["kind"] == "gateway_restart":
                continue
            self.assertGreaterEqual(fault["at"] - previous, 30)
            self.assertLessEqual(fault["at"] - previous, 120)
            previous = fault["at"]
            seen.add((fault["kind"], fault["target"]))
        self.assertEqual(seen, {("client_sse", client) for client in range(8)} |
                         {("runtime_link", conversation) for conversation in range(4)})
        self.assertEqual([fault["at"] for fault in faults if fault["kind"] == "gateway_restart"],
                         [1800, 3600, 5400, 7200])

    def test_pauses_are_seeded_bounded_and_do_not_overlap_a_restart_or_the_same_runtime(self):
        for rehearsal, seconds, interval in ((False, 7200, 1800), (True, 300, 300)):
            for seed in range(10):
                available = [0] * 4
                faults = schedule(seed, seconds, rehearsal)
                for fault in faults:
                    if fault["kind"] == "runtime_link":
                        self.assertNotEqual(fault["at"] % interval, 0)
                        duration = fault["pause_seconds"]
                        self.assertGreaterEqual(duration, 121)
                        self.assertLessEqual(duration, 150)
                        self.assertGreater(interval - fault["at"] % interval, duration + 60)
                        self.assertGreater(fault["at"], available[fault["target"]])
                        available[fault["target"]] = fault["at"] + duration + 60

    def test_eight_conversations_cover_sixteen_clients_and_eight_runtimes(self):
        faults = schedule(731, 7200, slots=8)
        seen = {(fault["kind"], fault["target"]) for fault in faults if fault["kind"] != "gateway_restart"}
        self.assertEqual(seen, {("client_sse", client) for client in range(16)} |
                         {("runtime_link", conversation) for conversation in range(8)})

    def test_the_24_hour_plan_restarts_every_2_hours_until_hour_12_then_leaves_one_12_hour_lifetime(self):
        restarts = restart_times(86400, rehearsal=False)
        self.assertEqual(restarts, [7200, 14400, 21600, 28800, 36000, 43200])
        self.assertEqual(restart_times(7200, rehearsal=False), [1800, 3600, 5400, 7200])
        self.assertEqual(restart_times(300, rehearsal=True), [300])
        faults = schedule(731, 86400, slots=8, restarts=restarts)
        self.assertEqual([fault["at"] for fault in faults if fault["kind"] == "gateway_restart"], restarts)
        available = [0] * 8
        for fault in faults:
            if fault["kind"] == "runtime_link":
                upcoming = [at for at in restarts if at >= fault["at"]]
                if upcoming:
                    self.assertGreater(upcoming[0] - fault["at"], fault["pause_seconds"] + 60)
                self.assertGreater(fault["at"], available[fault["target"]])
                available[fault["target"]] = fault["at"] + fault["pause_seconds"] + 60
        late = [fault for fault in faults if fault["kind"] == "runtime_link" and fault["at"] > 43200]
        self.assertGreater(len(late), 8 * 6, "link pauses go on through the restart-free half")

    def test_the_2_hour_plan_is_unchanged_by_an_explicit_restart_list(self):
        self.assertEqual(schedule(731, 7200, slots=4), schedule(731, 7200, slots=4,
                                                                restarts=restart_times(7200, rehearsal=False)))

    def test_one_observer_run_reports_rss_and_open_descriptors(self):
        self.assertEqual(process_sample("81234 57\n"), {"gateway_rss_bytes": 81234 * 1024, "open_fds": 57})
        for bad in ("", "81234", "0 57", "81234 0", "x 57"):
            with self.subTest(bad), self.assertRaises(AuditError):
                process_sample(bad)

    def test_one_query_reports_database_size_and_connections(self):
        self.assertEqual(database_sample("123456|11|10|3|0\n"),
                         {"database_bytes": 123456, "database_connections": 11, "gateway_connections": 10,
                          "gateway_idle_age_seconds": 3, "gateway_stale_connections": 0})
        # An idle pool holds no connection; the probe's own session keeps the second count above zero.
        sample = database_sample("123456|1|0|0|0")
        self.assertEqual((sample["gateway_connections"], sample["gateway_idle_age_seconds"]), (0, 0))
        for bad in ("", "123456", "123456|11", "123456|11|10", "123456|11|10|3", "0|11|10|3|0", "123456|0|0|0|0",
                    "123456|11|x|3|0", "123456|11|10|-1|0", "123456|11|10|3|x"):
            with self.subTest(bad), self.assertRaises(AuditError):
                database_sample(bad)

    # TEST: SOAK-001
    def test_the_query_reads_the_gateways_connections_idle_age(self):
        """D4 (9 Oct 2026): the oldest idle gateway connection, in any of Postgres's three idle states, and how many
        have been so for over STALE_SECONDS. The probe's own session never counts."""
        query = DATABASE_QUERY.lower()
        # The pool's own bound with no traffic: sqlx's idle_timeout (600 s) plus its reaper's period (600 s, and a
        # re-release resets sqlx's idle clock, not Postgres's state_change), plus 5 min.
        self.assertEqual(STALE_SECONDS, 1500)
        self.assertIn("now() - state_change", query)
        self.assertIn(f"interval '{STALE_SECONDS} seconds'", query)

    # TEST: SOAK-001
    def test_the_idle_age_counts_a_connection_left_in_a_failed_transaction(self):
        """Postgres has three idle states: 'idle', 'idle in transaction' and 'idle in transaction (aborted)'. A guard
        leaked on an error path holds the last one, the likeliest leak of all (the review of 10 Oct, P1)."""
        query = DATABASE_QUERY.lower()
        self.assertIn("state like 'idle%'", query)
        self.assertNotIn("state in (", query, "a list of states misses one")
        self.assertEqual(query.count("pid <> pg_backend_pid()"), 3, "every gateway column leaves the probe out")
        self.assertEqual(query.count("backend_type = 'client backend'"), 3)

    def test_a_vm_host_without_the_sandbox_network_or_a_credential_label_still_has_an_identity(self):
        record = {"Id": "b" * 64, "Config": {"Labels": {}},
                  "NetworkSettings": {"Networks": {"tqa-fc": {"IPAddress": "203.0.113.20"}}}}
        before = runtime_identity(record, "owned")
        self.assertEqual(before["addresses"], {"tqa-fc": "203.0.113.20"})
        self.assertIsNone(before["credential_binding"])
        record["NetworkSettings"]["Networks"]["tqa-fc"]["IPAddress"] = "203.0.113.21"
        self.assertNotEqual(runtime_identity(record, "owned"), before)

    def test_helpers_cannot_use_host_namespaces_tags_or_unbounded_targets(self):
        image, container = "sha256:" + "a" * 64, "b" * 64
        arguments = observer_arguments(image, container)
        self.assertIn("container:" + container, arguments)
        self.assertNotIn("--cap-add", arguments)
        self.assertNotIn("--privileged", arguments)
        self.assertNotIn("--mount", arguments)
        self.assertNotIn("--volume", arguments)
        self.assertEqual(arguments[arguments.index("--user") + 1], "1000:1000")
        self.assertEqual(arguments[arguments.index("--network") + 1], "none")
        for values in [("runtime:latest", container), (image, "host")]:
            with self.subTest(values=values), self.assertRaises(AuditError):
                observer_arguments(*values)

    def test_runtime_identity_includes_its_address_and_credential_binding(self):
        record = {"Id": "a" * 64, "Config": {"Labels": {"tqa.token_id": "binding"}},
                  "NetworkSettings": {"Networks": {"owned": {"IPAddress": "192.0.2.1"}}}}
        before = runtime_identity(record, "owned")
        record["NetworkSettings"]["Networks"]["owned"]["IPAddress"] = "192.0.2.2"
        self.assertNotEqual(runtime_identity(record, "owned"), before)
        record["NetworkSettings"]["Networks"]["owned"]["IPAddress"] = "192.0.2.1"
        record["Config"]["Labels"]["tqa.token_id"] = "replacement"
        self.assertNotEqual(runtime_identity(record, "owned"), before)


if __name__ == "__main__":
    unittest.main()
