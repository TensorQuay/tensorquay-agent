"""The shared GW-I-314 half-open case's parsing, bounds and checks, without a stack."""

import ipaddress
import json
import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import halfopen_case as case  # noqa: E402

GATEWAY, SANDBOX, OTHER = "192.0.2.10", "192.0.2.11", "198.51.100.7"
HEADER = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n"


def field(address, port):
    """An IPv4 address and port as /proc/net/tcp writes them: the address's bytes in host (little-endian) order."""
    return "{}:{:04X}".format(ipaddress.IPv4Address(address).packed[::-1].hex().upper(), port)


def row(local, remote, state="01", inode=1):
    return "   0: {} {} {} 00000000:00000000 00:00000000 00000000  1000        0 {} 1 0\n".format(
        field(*local), field(*remote), state, inode)


def observed(rows, fds, tcp6=""):
    return json.dumps({"fds": fds, "tcp": HEADER + "".join(rows), "tcp6": HEADER + tcp6})


PEER = (SANDBOX, 40664)
LISTENING = row(("0.0.0.0", 8080), ("0.0.0.0", 0), state="0A")
HELD = row((GATEWAY, 8080), PEER, inode=478192985)
LINK = row((GATEWAY, 8080), (SANDBOX, 51000), inode=2)
DATABASE = row((GATEWAY, 41000), (OTHER, 5432), inode=3)


class Parsing(unittest.TestCase):
    def test_an_ipv4_field_is_read_in_host_order(self):
        self.assertEqual(case.address("0100007F:1F90"), ("127.0.0.1", 8080))
        self.assertEqual(case.address(field(SANDBOX, 40664)), PEER)

    def test_an_ipv4_mapped_ipv6_field_is_read_as_ipv4(self):
        self.assertEqual(case.address("0000000000000000FFFF00000B0200C0:1F90"), (SANDBOX, 8080))

    def test_rows_carry_their_ends_state_and_inode(self):
        rows = case.sockets(HEADER + HELD + LISTENING)
        self.assertEqual(rows[0], {"local": (GATEWAY, 8080), "remote": PEER, "state": "01", "inode": "478192985"})
        self.assertEqual(rows[1]["state"], case.LISTEN)

    def test_a_sample_joins_both_tables_and_refuses_an_empty_reading(self):
        tcp6 = "   1: 0000000000000000FFFF00000B0200C0:1F90 0000000000000000FFFF00000B0200C0:9C40 01 0 0 0 0 0 9\n"
        self.assertEqual(len(case.sample(observed([HELD], 40, tcp6), 6)["sockets"]), 2)
        with self.assertRaises(AssertionError):
            case.sample(json.dumps({"fds": 0, "tcp": HEADER, "tcp6": HEADER}), 6)


class Bounds(unittest.TestCase):
    def test_the_held_bound_is_the_keepalive_plus_a_margin(self):
        self.assertEqual(case.KEEPALIVE_SECONDS, 90)
        self.assertGreater(case.HELD_SECONDS, case.KEEPALIVE_SECONDS)

    def test_the_whole_bound_covers_the_links_own_liveness(self):
        self.assertGreater(case.ALL_SECONDS, 30 + 90)
        self.assertGreaterEqual(case.ALL_SECONDS, case.HELD_SECONDS)

    def test_the_client_asks_once_and_then_stays_idle(self):
        self.assertIn("GET /healthz HTTP/1.1", case.CLIENT)
        self.assertIn("('gateway', 8080)", case.CLIENT)
        self.assertIn(case.PORT_FILE, case.CLIENT)
        compile(case.CLIENT, "client", "exec")
        compile(case.OBSERVER, "observer", "exec")


def snapshot(rows, fds, database=6):
    return case.sample(observed(rows, fds), database)


BEFORE = snapshot([LISTENING, HELD, LINK, DATABASE], 50)
BASELINE = 50 - 6 - 2


class Check(unittest.TestCase):
    def test_before_the_cut_the_level_leaves_out_the_sandbox_and_the_database(self):
        self.assertEqual(case.check_before(BEFORE, PEER, {SANDBOX}), BASELINE)

    def test_before_the_cut_the_connection_must_be_established(self):
        with self.assertRaisesRegex(AssertionError, "isn't established"):
            case.check_before(snapshot([LISTENING, LINK], 49), PEER, {SANDBOX})

    def run_watch(self, snapshots):
        """`watch` over a list of samples, five seconds apart on a fake clock."""
        clock = {"now": 0.0}
        samples = iter(snapshots)
        return case.watch(lambda: next(samples), PEER, {SANDBOX}, BASELINE, lambda: clock["now"],
                          lambda seconds: clock.update(now=clock["now"] + seconds))

    def test_a_connection_freed_by_keepalive_then_the_link_passes(self):
        held = [snapshot([LISTENING, HELD, LINK, DATABASE], 50)] * 18
        link_only = [snapshot([LISTENING, LINK, DATABASE], 49)] * 4
        clean = [snapshot([LISTENING, DATABASE], 48)]
        connection, everything, last = self.run_watch(held + link_only + clean)
        self.assertEqual((connection, everything), (90.0, 110.0))
        self.assertIn("freed 90 s after the cut", case.check(connection, everything, last, PEER, {SANDBOX}, BASELINE))

    def test_a_connection_held_to_the_bound_fails(self):
        held = [snapshot([LISTENING, HELD, DATABASE], 49)] * 40
        connection, everything, last = self.run_watch(held)
        self.assertEqual((connection, everything), (None, None))
        with self.assertRaisesRegex(AssertionError, "still holds the sandbox's idle connection"):
            case.check(connection, everything, last, PEER, {SANDBOX}, BASELINE)

    def test_a_connection_freed_after_its_own_bound_fails(self):
        late = [snapshot([LISTENING, HELD, DATABASE], 49)] * 26 + [snapshot([LISTENING, DATABASE], 48)]
        connection, everything, last = self.run_watch(late)
        self.assertEqual(connection, 130.0)
        with self.assertRaisesRegex(AssertionError, "ended 130 s after the cut"):
            case.check(connection, everything, last, PEER, {SANDBOX}, BASELINE)

    def test_descriptors_left_over_the_level_fail(self):
        leaked = [snapshot([LISTENING, DATABASE], 49)] * 40
        connection, everything, last = self.run_watch(leaked)
        self.assertEqual((connection, everything), (0.0, None))
        with self.assertRaisesRegex(AssertionError, "descriptors less the database 43"):
            case.check(connection, everything, last, PEER, {SANDBOX}, BASELINE)

    def test_the_watch_is_reported_whatever_it_saw(self):
        held = [snapshot([LISTENING, HELD, DATABASE], 49)] * 40
        connection, everything, last = self.run_watch(held)
        self.assertEqual(case.report(connection, everything, last, PEER, {SANDBOX}, BASELINE),
                         "GW-I-314 watch: the idle connection not ended within 150 s (bound 120 s); the sandbox's "
                         "sockets not all gone by 150 s (bound 150 s): 1 left; descriptors less the database 43 "
                         "(level without the sandbox 42)")
        freed = [snapshot([LISTENING, HELD, LINK, DATABASE], 50)] * 18 + [snapshot([LISTENING, DATABASE], 48)]
        connection, everything, last = self.run_watch(freed)
        self.assertEqual(case.report(connection, everything, last, PEER, {SANDBOX}, BASELINE),
                         "GW-I-314 watch: the idle connection ended 90 s after the cut (bound 120 s); the sandbox's "
                         "sockets all gone by 90 s (bound 150 s); descriptors less the database 42 (level without "
                         "the sandbox 42)")

    def test_more_database_connections_are_not_a_leak(self):
        connection, everything, _ = self.run_watch([snapshot([LISTENING, DATABASE], 52, database=10)])
        self.assertEqual((connection, everything), (0.0, 0.0))


def event(action, ident, seconds, code=None):
    """One `docker events --format '{{json .}}'` line for a sandbox container."""
    attributes = {"name": "tqa-sandbox-x", "tqa.conversation": "c1"}
    if code is not None:
        attributes["exitCode"] = str(code)
    return json.dumps({"Type": "container", "Action": action, "id": ident, "timeNano": int(seconds * 1e9),
                       "Actor": {"ID": ident, "Attributes": attributes}})


class Exits(unittest.TestCase):
    FIRST, SECOND = "9df5d5ef8944" + "a" * 52, "7c1e00b2d3f4" + "b" * 52

    def test_each_sandbox_says_when_it_started_died_and_with_what_code(self):
        lines = [event("start", self.FIRST, 100.0), event("die", self.FIRST, 102.4, 0),
                 event("start", self.SECOND, 103.0), event("die", self.SECOND, 250.0, 137)]
        self.assertEqual(case.exits(lines, {self.FIRST: ["2026-10-20T07:39:18Z runtime: exiting"]}), [
            "sandbox 9df5d5ef8944: started, died 2.4 s later, exit 0 (a deliberate exit); last log lines: "
            "2026-10-20T07:39:18Z runtime: exiting",
            "sandbox 7c1e00b2d3f4: started, died 147.0 s later, exit 137 (killed: SIGKILL, the case's kill or memory); "
            "last log lines: none read"])

    def test_a_sandbox_still_running_or_seen_only_dying_says_so(self):
        lines = [event("start", self.FIRST, 100.0), "", "not json", event("die", self.SECOND, 50.0, 1)]
        self.assertEqual(case.exits(lines, {}), [
            "sandbox 9df5d5ef8944: started, no die seen; last log lines: none read",
            "sandbox 7c1e00b2d3f4: start not seen, died, exit 1 (a crash); last log lines: none read"])

    def test_events_without_the_old_top_level_id_are_read_by_their_actor(self):
        # Docker 28 dropped the deprecated top-level "id", "status" and "from" (the first rerun, 20 Oct 15:56).
        lines = []
        for action, seconds, code in (("start", 100.0, None), ("die", 102.0, 0)):
            item = json.loads(event(action, self.FIRST, seconds, code))
            del item["id"]
            lines.append(json.dumps(item))
        self.assertEqual(case.exits(lines, {}), [
            "sandbox 9df5d5ef8944: started, died 2.0 s later, exit 0 (a deliberate exit); last log lines: none read"])

    def test_no_events_is_said(self):
        self.assertEqual(case.exits([], {}), ["no sandbox start or die events seen for the task"])



# GW-I-318: the second half-open case, a cut link reconnected in time, keeps its connection.
class Reconnect(unittest.TestCase):
    def test_the_bounds_reconnect_before_the_first_probe_and_watch_past_keepalive(self):
        # The reconnect comes before the gateway's first probe (60 s idle), and the watch runs past the probes' 90 s, so
        # the probes after the reconnect are answered and nothing may close the connection.
        self.assertLess(case.RECONNECT_AFTER_SECONDS, 60)
        self.assertGreater(case.KEPT_SECONDS, case.KEEPALIVE_SECONDS + case.RECONNECT_AFTER_SECONDS)
        self.assertIn("idle_seconds=", case.RECONNECT_SETTINGS)
        idle = int(case.RECONNECT_SETTINGS.split("=")[1])
        self.assertGreater(idle, case.KEPT_SECONDS + 120, "the gateway never retires the idle sandbox mid-case")

    def test_the_client_asks_again_on_the_same_socket_when_told(self):
        compile(case.RECONNECT_CLIENT, "client", "exec")
        self.assertEqual(case.RECONNECT_CLIENT.count("create_connection"), 1, "one socket, never a second")
        self.assertEqual(case.RECONNECT_CLIENT.count("GET /healthz HTTP/1.1"), 2)
        for name in (case.PORT_FILE, case.ASK_FILE, case.ANSWER_FILE):
            self.assertIn(name, case.RECONNECT_CLIENT)

    def test_the_restored_link_must_have_the_same_address(self):
        case.check_restored({"net-a": SANDBOX}, {"net-a": SANDBOX})
        with self.assertRaisesRegex(AssertionError, "tests nothing.*address changed"):
            case.check_restored({"net-a": SANDBOX}, {"net-a": OTHER})
        with self.assertRaisesRegex(AssertionError, "tests nothing"):
            case.check_restored({"net-a": SANDBOX}, {})

    def run_kept(self, snapshots):
        clock = {"now": 0.0}
        samples = iter(snapshots)
        return case.watch_kept(lambda: next(samples), PEER, lambda: clock["now"],
                               lambda seconds: clock.update(now=clock["now"] + seconds))

    def samples(self, count, rows):
        return [snapshot(rows, 50)] * count

    def test_a_connection_established_through_the_watch_is_kept(self):
        seen = self.run_kept(self.samples(40, [LISTENING, HELD, LINK, DATABASE]))
        self.assertEqual(seen["lost_at"], None)
        self.assertGreaterEqual(seen["watched"], case.KEPT_SECONDS)
        self.assertEqual(case.check_kept(seen, "HTTP/1.1 200 OK"),
                         "GW-I-318: the sandbox's idle keep-alive connection, its link cut and reconnected after 30 s, "
                         "stayed established for 150 s after the cut (past keepalive's 90 s) and answered a second "
                         "request on the same socket")

    def test_a_connection_closed_after_the_reconnect_fails_and_says_when(self):
        held = self.samples(20, [LISTENING, HELD, LINK, DATABASE])
        gone = self.samples(20, [LISTENING, LINK, DATABASE])
        seen = self.run_kept(held + gone)
        self.assertEqual(seen["lost_at"], 100.0)
        with self.assertRaisesRegex(AssertionError, "closed a live peer.*100 s after the cut"):
            case.check_kept(seen, "HTTP/1.1 200 OK")

    def test_a_connection_left_in_another_state_counts_as_lost(self):
        closing = row((GATEWAY, 8080), PEER, state="08", inode=478192985)  # CLOSE_WAIT
        seen = self.run_kept(self.samples(3, [LISTENING, HELD]) + self.samples(37, [LISTENING, closing]))
        self.assertEqual(seen["lost_at"], 15.0)
        self.assertEqual(seen["last_state"], "08")

    def test_an_unanswered_second_request_fails(self):
        seen = self.run_kept(self.samples(40, [LISTENING, HELD]))
        for answer in ("", "error: timed out", "HTTP/1.1 502 Bad Gateway"):
            with self.assertRaisesRegex(AssertionError, "second request"):
                case.check_kept(seen, answer)

    def test_the_watch_is_reported_whatever_it_saw(self):
        kept = self.run_kept(self.samples(40, [LISTENING, HELD]))
        self.assertIn("established in all", case.report_kept(kept, "HTTP/1.1 200 OK"))
        lost = self.run_kept(self.samples(5, [LISTENING, HELD]) + self.samples(35, [LISTENING]))
        self.assertIn("gone 25 s after the cut", case.report_kept(lost, ""))
        self.assertIn("no answer", case.report_kept(lost, ""))

if __name__ == "__main__":
    unittest.main()
