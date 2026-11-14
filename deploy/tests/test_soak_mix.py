"""SOAK-001's mix of agent teams and Desktop tasks (ADR-0034, ADR-0039 §10): off unless asked, today's soak unchanged;
the scripts the fake model serves; the lanes that drive one team and one Desktop task at a time; and the audit's
counts, each of which must come back to its floor of 0."""
import copy
import json
import types
import unittest
import urllib.error
from unittest import mock

import soak_mix
import soak_mix_audit as mix_audit
import soak_stack
from soak_audit import AuditError
from soak_scenario import RESPONSES, scenario

LEAD, HELPER_A, HELPER_B, DESK = "lead-1", "helper-a", "helper-b", "desk-1"
DESKTOP_IMAGE = "sha256:" + "d" * 64


def zero():
    return {name: 0 for name in mix_audit.COUNTS}


class OffTests(unittest.TestCase):
    """Without --teams-every and --desktops-every, today's soak is the same byte for byte."""

    def test_no_lane_scenario_without_the_mix(self):
        self.assertIsNone(soak_mix.lane_scenario(teams=False, desktops=False))

    def test_the_gateway_file_is_unchanged_without_teams(self):
        text = "listen='0.0.0.0:8080'\n[sandbox]\nmax_live=16\n"
        self.assertEqual(soak_mix.gateway_text(text, teams=False), text)

    def test_max_live_and_listeners_are_todays_without_the_mix(self):
        for slots in (4, 8):
            self.assertEqual(soak_mix.max_live(slots, teams=False, desktops=False), soak_stack.max_live(slots))
        self.assertEqual(soak_mix.listeners(teams=False), 0)

    def test_no_mix_is_the_default_and_its_options_are_none(self):
        options = soak_mix.MixOptions()
        self.assertFalse(options.on)
        self.assertEqual(soak_mix.check_mix(options, backend="docker", model="fake"), None)


class OptionTests(unittest.TestCase):
    def test_a_period_is_whole_minutes_from_two_to_sixty(self):
        for good in (2, 10, 60):
            soak_mix.check_mix(soak_mix.MixOptions(teams_every=good), backend="docker", model="fake")
        for bad in (0, 1, 61):
            with self.assertRaises(AuditError):
                soak_mix.check_mix(soak_mix.MixOptions(desktops_every=bad), backend="docker", model="fake")

    def test_the_mix_runs_on_docker_with_the_fake_model_only(self):
        options = soak_mix.MixOptions(teams_every=10)
        with self.assertRaisesRegex(AuditError, "fake model"):
            soak_mix.check_mix(options, backend="docker", model="real")
        with self.assertRaisesRegex(AuditError, "Docker"):
            soak_mix.check_mix(options, backend="firecracker", model="fake")


class ScenarioTests(unittest.TestCase):
    """The lanes' own scripted model (MIX_SERVICE, route MIX_ROUTE): the slots' model and script stay today's."""

    def mixed(self, teams=True, desktops=True):
        return soak_mix.lane_scenario(teams=teams, desktops=desktops)

    def test_the_lanes_scenario_selects_by_prompt_within_the_models_reply_cap(self):
        mixed = self.mixed()
        self.assertEqual((mixed["model"], mixed["select"]), ("fixture", "prompt"))
        self.assertLessEqual(len(mixed["responses"]), soak_mix.MAX_RESPONSES,
                             "the fake model refuses a scenario of more than 128 replies")
        self.assertEqual(soak_mix.MAX_RESPONSES, 128)

    def test_every_mix_reply_names_its_phrase_and_no_phrase_holds_another(self):
        phrases = {reply["when"] for reply in self.mixed()["responses"]}
        self.assertEqual(phrases, {soak_mix.TEAM_PHRASE, soak_mix.DESK_PHRASE}
                         | {"You are " + name for name in soak_mix.HELPERS})
        for phrase in phrases:
            others = phrases - {phrase}
            self.assertFalse(any(phrase in other or other in phrase for other in others), phrase)

    def test_the_lead_starts_two_own_copy_helpers_asks_reads_and_ends_with_done(self):
        lead = [reply["events"][0] for reply in self.mixed()["responses"] if reply["when"] == soak_mix.TEAM_PHRASE]
        calls = [event.get("name") for event in lead]
        self.assertEqual(calls[:3], ["start_member", "start_member", "ask_all"])
        self.assertEqual(calls[-2], "done")
        self.assertEqual(lead[-1]["type"], "text",
                         "the last reply repeats: after done the lead ends its turn with text, as the agents suite's "
                         "lead does (the 2 h rehearsal of 12 Nov 2026 looped on a repeated done)")
        self.assertEqual(set(lead[-2]["arguments"]), {"summary"}, "done's schema is strict: its summary alone")
        self.assertIn("read_reports", calls)
        starts = [event["arguments"] for event in lead if event.get("name") == "start_member"]
        self.assertEqual([start["role"] for start in starts], list(soak_mix.HELPERS))
        self.assertEqual({start["workspace"] for start in starts}, {"own_copy"})

    def test_each_helper_writes_a_file_and_reports_it(self):
        for name in soak_mix.HELPERS:
            own = [reply["events"][0] for reply in self.mixed()["responses"] if reply["when"] == "You are " + name]
            self.assertEqual([event.get("name") for event in own][:2], ["shell", "send_to_lead"])
            self.assertEqual(own[1]["arguments"]["type"], "report")
            self.assertEqual(own[1]["arguments"]["files"], ["result.md"])
            self.assertEqual(own[-1]["type"], "text")

    def test_the_desktop_task_acts_and_observes(self):
        desk = [reply["events"][0] for reply in self.mixed()["responses"] if reply["when"] == soak_mix.DESK_PHRASE]
        actions = [event["arguments"]["action"] for event in desk if event.get("name") == "computer"]
        self.assertEqual(actions, ["key", "type", "key", "wait", "observe"])
        self.assertEqual(desk[-1]["type"], "text")

    def test_only_the_lanes_asked_for_are_scripted(self):
        phrases = {reply["when"] for reply in self.mixed(teams=True, desktops=False)["responses"]}
        self.assertNotIn(soak_mix.DESK_PHRASE, phrases)
        phrases = {reply["when"] for reply in self.mixed(teams=False, desktops=True)["responses"]}
        self.assertNotIn(soak_mix.TEAM_PHRASE, phrases)

    def test_the_lanes_route_is_the_slots_route_on_the_lanes_model(self):
        local = "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='http://fake-model:8080/v1'\n" \
                "model='fixture'\nauth={kind='bearer'}\nkey_ref='secrets/local'\n"
        text = soak_mix.models_text(local + "context_window=1000000\nstream_usage=true\n")
        import tomllib
        routes = {route["name"]: route for route in tomllib.loads(text)["routes"]}
        self.assertEqual(routes["mix"]["base_url"], "http://fake-model-mix:8080/v1")
        self.assertEqual({k: v for k, v in routes["mix"].items() if k not in ("name", "base_url")},
                         {k: v for k, v in routes["local"].items() if k not in ("name", "base_url")})

    def test_the_lanes_model_is_the_demos_fake_model_with_its_own_read_only_scenario(self):
        service = soak_mix.lane_service("/installation/mix.json", "sha256:" + "f" * 64, "/repo/deploy/compose/demo.yml")
        self.assertEqual(service["extends"], {"file": "/repo/deploy/compose/demo.yml", "service": "fake-model"})
        self.assertEqual(service["command"], ["--scenario", "/scenarios/mix.json"])
        self.assertEqual(service["volumes"], [{"type": "bind", "source": "/installation/mix.json",
                                               "target": "/scenarios/mix.json", "read_only": True}])


class LauncherTests(unittest.TestCase):
    def test_teams_turn_the_switch_on(self):
        text = soak_mix.gateway_text("listen='x'\n", teams=True)
        self.assertTrue(text.endswith("\n[agent_teams]\nenabled = true\n"))
        self.assertEqual(soak_mix.listeners(teams=True), 1)

    def test_the_mix_makes_room_for_a_team_and_a_desktop(self):
        self.assertEqual(soak_mix.max_live(8, teams=True, desktops=True), 20)
        self.assertEqual(soak_mix.max_live(8, teams=True, desktops=False), 19)
        self.assertEqual(soak_mix.max_live(4, teams=False, desktops=True), 9)

    def test_a_desktop_lane_needs_the_desktop_image_and_records_its_id_and_digests(self):
        with self.assertRaisesRegex(AuditError, "TQA_DESKTOP_TEST_IMAGE"):
            soak_mix.desktop_image({}, desktops=True, inspect=lambda reference: {})
        self.assertIsNone(soak_mix.desktop_image({"TQA_DESKTOP_TEST_IMAGE": "x"}, desktops=False,
                                                 inspect=lambda reference: {}))
        record = {"Id": DESKTOP_IMAGE, "RepoDigests": ["tqa-runtime@sha256:" + "e" * 64]}
        pinned = soak_mix.desktop_image({"TQA_DESKTOP_TEST_IMAGE": "tqa-runtime:desktop"}, desktops=True,
                                        inspect=lambda reference: record)
        self.assertEqual(pinned, {"id": DESKTOP_IMAGE, "repo_digests": record["RepoDigests"],
                                  "reference": "tqa-runtime:desktop"})

    def test_a_pinned_desktop_must_be_in_the_gateway_file_by_id(self):
        text = '\n[runtime.builtin_images]\ndesktop = "{}"\nminimal = "sha256:1"\n'.format(DESKTOP_IMAGE)
        soak_mix.check_pinned(text, DESKTOP_IMAGE)
        with self.assertRaisesRegex(AuditError, "pins"):
            soak_mix.check_pinned(text.replace("desktop =", "workstation ="), DESKTOP_IMAGE)
        with self.assertRaisesRegex(AuditError, "pins"):
            soak_mix.check_pinned('[runtime.builtin_images]\ndesktop = "tqa-runtime:desktop"\n', DESKTOP_IMAGE)

    def test_max_sessions_is_read_from_the_admin_limits(self):
        limits = {"items": [{"key": "admission.slots", "value": 20}, {"key": "desktop.max_sessions", "value": 2}]}
        self.assertEqual(soak_mix.max_sessions(limits), 2)
        with self.assertRaises(AuditError):
            soak_mix.max_sessions({"items": []})


class AuditCountTests(unittest.TestCase):
    def test_the_database_counts_parse_into_their_names_and_the_copies_ids(self):
        text = "teams|1\nteam_log_rows|9\nworkspace_copies|2\npending_team_turns|1\nreservations|0\n" \
               "copy|c-1\ncopy|c-2\nworkspace|w-1\n"
        parsed = mix_audit.parse_database(text)
        self.assertEqual(parsed["counts"], {"teams": 1, "team_log_rows": 9, "workspace_copies": 2,
                                            "pending_team_turns": 1, "reservations": 0})
        self.assertEqual(parsed["copies"], {"c-1", "c-2"})
        self.assertEqual(parsed["workspaces"], {"w-1"})
        with self.assertRaises(AuditError):
            mix_audit.parse_database("teams|x\n")

    def test_the_query_counts_queued_team_turns_and_every_reservation(self):
        query = mix_audit.DATABASE_QUERY
        self.assertIn("execution_status = 'queued'", query)
        self.assertIn("team-wake:%", query)
        self.assertIn("team-start:%", query)
        self.assertIn("runtime_token_reservations", query)
        self.assertIn("kind = 'helper_copy'", query)

    def record(self, conversation, image="sha256:" + "a" * 64, state="running", tmpfs=None):
        return {"Id": conversation + "-container", "Image": image, "State": {"Status": state},
                "Config": {"Labels": {"tqa.conversation": conversation}},
                "HostConfig": {"Tmpfs": tmpfs or {}}}

    def test_desktops_processes_and_profiles_come_from_desktop_sandboxes_only(self):
        browser = {"/run/tqa-browser": "size=512m", "/dev/shm": "size=1g"}
        records = [self.record("slot-0"), self.record(DESK, DESKTOP_IMAGE, tmpfs=browser),
                   self.record("desk-2", DESKTOP_IMAGE, state="exited", tmpfs=browser)]
        tops = {DESK + "-container": 14}
        counts = mix_audit.desktop_counts(records, DESKTOP_IMAGE, tops)
        self.assertEqual(counts, {"desktops_in_use": 1, "desktop_processes": 14, "profile_folders": 2})
        self.assertEqual(mix_audit.desktop_counts(records, None, tops),
                         {"desktops_in_use": 0, "desktop_processes": 0, "profile_folders": 0})

    def test_helper_sandboxes_are_those_of_helpers_the_lanes_saw_in_any_state(self):
        records = [self.record(HELPER_A), self.record(HELPER_B, state="exited"), self.record("slot-0")]
        self.assertEqual(mix_audit.helper_sandboxes(records, {HELPER_A, HELPER_B}), 2)

    def test_screenshots_and_copy_folders_come_from_the_storage_listing(self):
        listing = "screens/desk-1/a.png\nscreens/desk-1/b.png\nscreens/desk-2/c.png\nscreens/desk-2/notes.txt\n" \
                  "workspaces/w-1\nworkspaces/c-1\nworkspaces/c-9\n"
        files = mix_audit.parse_storage(listing)
        self.assertEqual(files["screenshot_files"], 3)
        self.assertEqual(files["screens"], {"desk-1": 2, "desk-2": 1})
        self.assertEqual(files["workspace_folders"], {"w-1", "c-1", "c-9"})
        self.assertEqual(mix_audit.copy_folders(files["workspace_folders"], standard={"w-1"}), 2,
                         "a folder with no standard workspace row is a copy's, its row purged or not")

    def test_a_sample_over_max_sessions_or_two_helpers_fails(self):
        counts = zero()
        self.assertEqual(mix_audit.sample_problems(counts, max_sessions=2), [])
        counts.update(desktops_in_use=3)
        self.assertIn("desktops in use", " ".join(mix_audit.sample_problems(counts, max_sessions=2)))
        counts = dict(zero(), helper_sandboxes=3)
        self.assertIn("helper sandboxes", " ".join(mix_audit.sample_problems(counts, max_sessions=2)))


class EndCheckTests(unittest.TestCase):
    """The end of the run: every new count back to its floor of 0. Each negative control is a leak that must fail."""

    def test_a_clean_end_passes(self):
        self.assertEqual(mix_audit.end_problems(zero()), [])

    def test_a_leaked_helper_sandbox_fails_the_run(self):
        self.assertEqual(mix_audit.end_problems(dict(zero(), helper_sandboxes=1)),
                         ["helper_sandboxes: 1 at the end, its floor 0"])

    def test_a_leaked_desktop_process_fails_the_run(self):
        self.assertEqual(mix_audit.end_problems(dict(zero(), desktop_processes=3)),
                         ["desktop_processes: 3 at the end, its floor 0"])

    def test_every_count_has_its_own_check(self):
        for name in mix_audit.COUNTS:
            self.assertEqual(len(mix_audit.end_problems(dict(zero(), **{name: 1}))), 1, name)

    def test_a_missing_count_fails_rather_than_passes(self):
        counts = zero()
        del counts["screenshot_files"]
        self.assertIn("screenshot_files", " ".join(mix_audit.end_problems(counts)))

    def test_a_team_settles_only_once_its_sandboxes_rows_folders_and_turns_are_gone(self):
        sandboxes = {"slot-0": 1}
        self.assertEqual(mix_audit.team_leftovers(zero(), sandboxes, {LEAD, HELPER_A, HELPER_B}), [])
        self.assertEqual(mix_audit.team_leftovers(zero(), {HELPER_A: 1}, {LEAD, HELPER_A, HELPER_B}),
                         ["conversation helper-a still has 1 sandbox"])
        for name in ("helper_sandboxes", "workspace_copies", "copy_folders", "team_log_rows", "teams",
                     "pending_team_turns"):
            self.assertEqual(len(mix_audit.team_leftovers(dict(zero(), **{name: 1}), {}, {LEAD})), 1, name)

    def test_a_desktop_settles_only_once_its_sandbox_processes_profile_and_pictures_are_gone(self):
        self.assertEqual(mix_audit.desktop_leftovers(zero(), {}, {}, DESK), [])
        self.assertEqual(mix_audit.desktop_leftovers(zero(), {DESK: 1}, {}, DESK),
                         ["conversation desk-1 still has 1 sandbox"])
        self.assertEqual(mix_audit.desktop_leftovers(zero(), {}, {DESK: 2}, DESK),
                         ["conversation desk-1 still has 2 screenshot files"])
        for name in ("desktops_in_use", "desktop_processes", "profile_folders"):
            self.assertEqual(len(mix_audit.desktop_leftovers(dict(zero(), **{name: 1}), {}, {}, DESK)), 1, name)


class FakeApi:
    """The stack as a lane sees it: requests, deletes, stored events and the audit's probe."""

    def __init__(self):
        self.created, self.messages, self.deleted, self.log = [], [], [], []
        self.team = None
        self.events = []
        self.probe = {"counts": zero(), "sandboxes": {}, "screens": {}}
        self.failure = None

    def request(self, path, body=None):
        if self.failure is not None:
            failure, self.failure = self.failure, None
            raise failure
        if path == "/api/v1/conversations":
            identity = DESK if body.get("template") == "desktop" else LEAD
            self.created.append(body)
            return {"id": identity}
        if path.endswith("/messages"):
            self.messages.append((path, body))
            return {}
        if path.endswith("/team"):
            if self.team is None:
                raise urllib.error.HTTPError(path, 404, "no_team", {}, None)
            return self.team
        if "/team/messages" in path:
            return {"items": self.team_messages, "last_seq": 9, "more": False}
        raise AssertionError(path)

    def delete(self, path):
        self.deleted.append(path)

    def stored(self, conversation):
        yield list(self.events)

    def mix_probe(self, helpers):
        return copy.deepcopy(self.probe)


def ended_team(stop_reason="done", helpers=soak_mix.HELPERS):
    return {"id": "team-1", "state": "ended", "stop_reason": stop_reason,
            "helpers": [{"name": name, "conversation": "helper-" + name[-1]} for name in helpers]}


def messages(reports=soak_mix.HELPERS, ended=1):
    items = [{"seq": index, "type": "report", "from": {"kind": "member", "name": name}}
             for index, name in enumerate(reports)]
    return items + [{"seq": 50 + index, "type": "ended", "from": {"kind": "gateway"}} for index in range(ended)]


class LaneTests(unittest.TestCase):
    def lane(self, kind, every=600):
        api = FakeApi()
        lane = (soak_mix.TeamLane if kind == "team" else soak_mix.DesktopLane)(api, every, start=0.0,
                                                                               log=lambda *a, **k: None)
        return lane, api

    def drive(self, lane, times, paused=False, draining=False):
        for now in times:
            lane.step(now, paused=paused, draining=draining)

    def test_a_team_starts_a_minute_in_and_its_message_names_the_phrase(self):
        lane, api = self.lane("team")
        self.drive(lane, [10, 59])
        self.assertEqual(api.created, [])
        self.drive(lane, [60])
        self.assertEqual(api.created, [{"title": "Soak team 0", "model_route": "mix"}])
        self.assertIn(soak_mix.TEAM_PHRASE, api.messages[0][1]["text"])
        self.assertFalse(lane.idle())

    def test_a_done_team_is_judged_deleted_with_its_helpers_and_settled(self):
        lane, api = self.lane("team")
        self.drive(lane, [60, 70])
        api.team, api.team_messages = ended_team(), messages()
        self.drive(lane, [80])
        self.assertEqual(api.deleted, ["/api/v1/conversations/lead-1?with_helpers=true"])
        api.probe["sandboxes"] = {"helper-t": 1}
        self.drive(lane, [90])
        self.assertFalse(lane.idle(), "a helper's sandbox still there: not settled")
        api.probe["sandboxes"] = {}
        self.drive(lane, [100])
        self.assertTrue(lane.idle())
        record = lane.records[0]
        self.assertEqual((record["stop_reason"], record["helpers"], record["reports"]),
                         ("done", sorted(soak_mix.HELPERS), 2))

    def test_the_next_team_waits_for_its_period(self):
        lane, api = self.lane("team", every=600)
        self.drive(lane, [60, 70])
        api.team, api.team_messages = ended_team(), messages()
        self.drive(lane, [80, 90, 600])
        self.assertEqual(len(api.created), 1)
        self.drive(lane, [660])
        self.assertEqual(len(api.created), 2)

    def test_a_team_that_ends_otherwise_fails_the_run(self):
        for team, items, why in ((ended_team("idle"), messages(), "idle"),
                                 (ended_team(helpers=("soak-analyst",)), messages(), "helpers"),
                                 (ended_team(), messages(reports=("soak-analyst",)), "report"),
                                 (ended_team(), messages(ended=0), "end note")):
            lane, api = self.lane("team")
            self.drive(lane, [60])
            api.team, api.team_messages = team, items
            with self.assertRaisesRegex(AuditError, why):
                self.drive(lane, [70])

    def test_a_conversation_that_never_ends_fails_after_its_bound(self):
        lane, api = self.lane("team")
        self.drive(lane, [60, 60 + soak_mix.CONVERSATION_SECONDS - 1])
        with self.assertRaisesRegex(AuditError, "did not end"):
            self.drive(lane, [61 + soak_mix.CONVERSATION_SECONDS])

    def test_a_leak_after_the_delete_fails_after_the_settle_bound(self):
        lane, api = self.lane("team")
        self.drive(lane, [60])
        api.team, api.team_messages = ended_team(), messages()
        self.drive(lane, [70])
        api.probe["counts"]["helper_sandboxes"] = 1
        self.drive(lane, [70 + soak_mix.SETTLE_SECONDS - 1])
        with self.assertRaisesRegex(AuditError, "helper_sandboxes"):
            self.drive(lane, [71 + soak_mix.SETTLE_SECONDS])

    def test_nothing_is_sent_while_paused_and_nothing_new_starts_in_the_drain(self):
        lane, api = self.lane("team")
        self.drive(lane, [60, 120], paused=True)
        self.drive(lane, [180], draining=True)
        self.assertEqual(api.created, [])
        self.assertTrue(lane.idle())

    def test_a_transient_failure_is_tried_again_and_a_refusal_is_not(self):
        lane, api = self.lane("team")
        self.drive(lane, [60])
        api.failure = urllib.error.HTTPError("x", 503, "unavailable", {}, None)
        self.drive(lane, [70])
        api.team, api.team_messages = ended_team(), messages()
        self.drive(lane, [80])
        self.assertEqual(len(api.deleted), 1)
        lane, api = self.lane("team")
        self.drive(lane, [60])
        api.failure = urllib.error.HTTPError("x", 422, "unknown_reference", {}, None)
        with self.assertRaises(urllib.error.HTTPError):
            self.drive(lane, [70])

    def test_a_desktop_task_completes_with_pictures_is_deleted_and_settles(self):
        lane, api = self.lane("desktop")
        self.drive(lane, [60])
        self.assertEqual(api.created, [{"title": "Soak desktop 0", "model_route": "mix", "template": "desktop"}])
        self.assertIn(soak_mix.DESK_PHRASE, api.messages[0][1]["text"])
        api.events = [{"seq": 1, "event": {"type": "screen.capture"}}, {"seq": 2, "event": {"type": "screen.capture"}}]
        self.drive(lane, [70])
        self.assertEqual(api.deleted, [])
        api.events.append({"seq": 3, "event": {"type": "run.completed"}})
        api.probe["screens"] = {DESK: 2}
        self.drive(lane, [80])
        self.assertEqual(api.deleted, ["/api/v1/conversations/desk-1"])
        self.drive(lane, [90])
        self.assertFalse(lane.idle(), "its pictures are still there")
        api.probe["screens"] = {}
        self.drive(lane, [100])
        self.assertTrue(lane.idle())
        self.assertEqual(lane.records[0]["captures"], 2)
        self.assertEqual(lane.records[0]["pictures"], 2)

    def full_disk_lane(self, windows, wall=1005.0):
        """A Desktop lane opened at wall-clock `wall` while the soak's disk-full windows read `windows`."""
        lane, api = self.lane("desktop")
        lane.disk_windows, lane.wall = (lambda: windows), (lambda: wall)
        self.drive(lane, [60])
        return lane, api

    def test_a_desktop_task_in_a_full_disk_window_may_keep_no_picture_when_the_product_said_so(self):
        # The screen store drops a capture it cannot keep and the run goes on (CORE-U-083); the result says so.
        lane, api = self.full_disk_lane([[1000.0, float("inf")]])
        api.events = [{"seq": 1, "event": {"type": "tool.start"}},
                      {"seq": 2, "event": {"type": "tool.result", "evidence": {"before": True, "after": False}}},
                      {"seq": 3, "event": {"type": "run.completed"}}]
        self.drive(lane, [70])
        self.assertEqual(api.deleted, ["/api/v1/conversations/desk-1"])
        self.drive(lane, [80])
        self.assertTrue(lane.idle(), "it settles as any Desktop task does")
        record = lane.records[0]
        self.assertEqual((record["pictures"], record["captures"], record["tool_calls"]), (0, 0, 1))
        self.assertEqual(record["pictures_not_kept"], "disk full")
        self.assertEqual(record["disk_full_window"], [1000.0, None])
        self.assertEqual(record["conversation"], DESK)

    def test_a_full_disk_window_that_closed_is_still_the_window_within_its_recovery(self):
        lane, api = self.full_disk_lane([[800.0, 900.0]], wall=900.0 + soak_mix.DISK_RECOVERY_SECONDS)
        api.events = [{"seq": 1, "event": {"type": "tool.result", "evidence": {"after": False}}},
                      {"seq": 2, "event": {"type": "run.completed"}}]
        self.drive(lane, [70])
        self.assertEqual(lane.record["disk_full_window"], [800.0, 900.0])

    def test_no_picture_outside_a_full_disk_window_still_fails_the_run(self):
        for windows in ([], [[100.0, 200.0]]):
            lane, api = self.full_disk_lane(windows)
            api.events = [{"seq": 1, "event": {"type": "tool.result", "evidence": {"after": False}}},
                          {"seq": 2, "event": {"type": "run.completed"}}]
            with self.assertRaisesRegex(AuditError, "kept no picture"):
                self.drive(lane, [70])

    def test_no_picture_in_a_full_disk_window_without_the_products_record_fails_the_run(self):
        # The product must have said so: a result whose evidence reads all kept, or none, is no dropped capture.
        for events in ([{"seq": 1, "event": {"type": "run.completed"}}],
                       [{"seq": 1, "event": {"type": "tool.result", "evidence": {"before": True, "after": True}}},
                        {"seq": 2, "event": {"type": "run.completed"}}]):
            lane, api = self.full_disk_lane([[1000.0, float("inf")]])
            api.events = events
            with self.assertRaisesRegex(AuditError, "kept no picture.*said nothing of a dropped capture"):
                self.drive(lane, [70])

    def test_pictures_kept_in_a_full_disk_window_are_an_ordinary_record(self):
        lane, api = self.full_disk_lane([[1000.0, float("inf")]])
        api.events = [{"seq": 1, "event": {"type": "screen.capture"}}, {"seq": 2, "event": {"type": "run.completed"}}]
        api.probe["screens"] = {DESK: 1}
        self.drive(lane, [70])
        self.assertNotIn("pictures_not_kept", lane.record)

    def test_the_window_of_the_13_november_run(self):
        # Desktop task 71 opened 3 ms after a 105 s disk_full fault started and ended 20 s later, inside it.
        windows = [[1794592702.9, 1794592737.1], [1794606860.8778, float("inf")]]
        self.assertEqual(soak_mix.disk_full_window(1794606860.8805, 1794606881.0, windows), windows[1])
        self.assertIsNone(soak_mix.disk_full_window(1794600000.0, 1794600020.0, windows))

    def test_a_desktop_task_that_fails_or_keeps_no_picture_fails_the_run(self):
        lane, api = self.lane("desktop")
        self.drive(lane, [60])
        api.events = [{"seq": 1, "event": {"type": "run.failed"}}]
        with self.assertRaisesRegex(AuditError, "failed"):
            self.drive(lane, [70])
        lane, api = self.lane("desktop")
        self.drive(lane, [60])
        api.events = [{"seq": 1, "event": {"type": "run.completed"}}]
        with self.assertRaisesRegex(AuditError, "picture"):
            self.drive(lane, [70])


class MixRunTests(unittest.TestCase):
    def test_off_has_no_lanes_and_reports_nothing(self):
        mix = soak_mix.MixRun(soak_mix.MixOptions(), api=FakeApi(), log=lambda *a, **k: None)
        mix.start(0.0, {"items": []})
        self.assertEqual(mix.lanes, [])
        self.assertTrue(mix.idle())
        self.assertIsNone(mix.finish())

    def test_the_start_must_be_clean_and_the_end_back_at_its_floor(self):
        api = FakeApi()
        mix = soak_mix.MixRun(soak_mix.MixOptions(teams_every=10, desktops_every=6), api=api,
                              log=lambda *a, **k: None, clock=lambda: 0.0, sleep=lambda seconds: None)
        mix.start(0.0, {"items": [{"key": "desktop.max_sessions", "value": 2}]})
        self.assertEqual(len(mix.lanes), 2)
        summary = mix.finish()
        self.assertEqual(summary["end"], zero())
        self.assertEqual(summary["start"], zero())
        self.assertEqual(summary["max_sessions"], 2)
        self.assertEqual(summary["teams"]["conversations"], 0)

    def test_a_leak_at_the_end_fails_after_the_settle_bound(self):
        api = FakeApi()
        clock = [0.0]
        mix = soak_mix.MixRun(soak_mix.MixOptions(desktops_every=6), api=api, log=lambda *a, **k: None,
                              clock=lambda: clock[0], sleep=lambda seconds: clock.__setitem__(0, clock[0] + seconds))
        mix.start(0.0, {"items": [{"key": "desktop.max_sessions", "value": 2}]})
        api.probe["counts"]["desktop_processes"] = 4
        with self.assertRaisesRegex(AuditError, "desktop_processes: 4 at the end"):
            mix.finish()
        self.assertGreaterEqual(clock[0], soak_mix.SETTLE_SECONDS)

    def test_a_sample_over_its_limit_fails_at_once(self):
        api = FakeApi()
        mix = soak_mix.MixRun(soak_mix.MixOptions(desktops_every=6), api=api, log=lambda *a, **k: None)
        mix.start(0.0, {"items": [{"key": "desktop.max_sessions", "value": 2}]})
        api.probe["counts"]["desktops_in_use"] = 3
        with self.assertRaisesRegex(AuditError, "desktops in use"):
            mix.sample()

    def test_a_dirty_start_fails(self):
        api = FakeApi()
        api.probe["counts"]["teams"] = 1
        mix = soak_mix.MixRun(soak_mix.MixOptions(teams_every=10), api=api, log=lambda *a, **k: None)
        with self.assertRaisesRegex(AuditError, "before the first"):
            mix.start(0.0, {"items": [{"key": "desktop.max_sessions", "value": 2}]})


class ProbeTests(unittest.TestCase):
    """MixProbe gathers the raw listings from the stack's engine and database; each through the stack's own bounded
    commands."""

    def test_the_probe_reads_containers_database_tops_and_storage(self):
        browser = {"/run/tqa-browser": "size=512m"}
        records = {
            "c1": {"Id": "c1", "Image": "sha256:" + "a" * 64, "State": {"Status": "running"},
                   "Config": {"Labels": {"tqa.conversation": HELPER_A}}, "HostConfig": {"Tmpfs": {}}},
            "c2": {"Id": "c2", "Image": DESKTOP_IMAGE, "State": {"Status": "running"},
                   "Config": {"Labels": {"tqa.conversation": DESK}}, "HostConfig": {"Tmpfs": browser}}}
        stack = types.SimpleNamespace(env={"TQA_SANDBOX_NETWORK": "net", "TQA_STORAGE_ROOT": "/s"}, database="db",
                                      inspect=lambda identity: records[identity])

        def command(arguments, timeout=90):
            if arguments[:2] == ["docker", "ps"]:
                return "c1\nc2"
            if arguments[:2] == ["docker", "top"]:
                return "UID PID\nu 1\nu 2\nu 3"
            if arguments[:2] == ["docker", "exec"]:
                return "teams|0\nteam_log_rows|0\nworkspace_copies|1\npending_team_turns|0\nreservations|0\n" \
                       "copy|c-1\nworkspace|w-1\n"
            raise AssertionError(arguments)
        stack.command = command
        stack.helper = lambda arguments: "screens/desk-1/a.png\nworkspaces/w-1\nworkspaces/c-1\n"
        probe = mix_audit.MixProbe(stack, DESKTOP_IMAGE, toybox="toybox")
        result = probe({HELPER_A})
        self.assertEqual(result["counts"], {"helper_sandboxes": 1, "workspace_copies": 1, "copy_folders": 1,
                                            "team_log_rows": 0, "teams": 0, "pending_team_turns": 0,
                                            "reservations": 0, "desktops_in_use": 1, "desktop_processes": 3,
                                            "profile_folders": 1, "screenshot_files": 1})
        self.assertEqual(result["sandboxes"], {HELPER_A: 1, DESK: 1})
        self.assertEqual(result["screens"], {DESK: 1})

    def test_the_storage_listing_mounts_the_root_read_only_without_network(self):
        stack = types.SimpleNamespace(env={"TQA_STORAGE_ROOT": "/srv/storage"})
        arguments = mix_audit.storage_arguments(stack, "toybox")
        self.assertIn("--network", arguments)
        self.assertIn("none", arguments)
        self.assertIn("type=bind,source=/srv/storage,target=/storage,readonly", arguments)


class BackgroundTests(unittest.TestCase):
    """The lanes run on their own thread, so the engine and database calls of their probes never hold the loop that
    drains the clients' events; the loop raises what the thread met."""

    def mix(self, api):
        mix = soak_mix.MixRun(soak_mix.MixOptions(teams_every=10), api=api, log=lambda *a, **k: None)
        mix.start(0.0, {"items": [{"key": "desktop.max_sessions", "value": 2}]})
        return mix

    def test_off_starts_no_thread(self):
        mix = soak_mix.MixRun(soak_mix.MixOptions(), api=FakeApi(), log=lambda *a, **k: None)
        mix.begin(lambda: 0.0, lambda: False, lambda: False, period=0.01)
        self.assertIsNone(mix.thread)
        mix.check()
        mix.end()

    def test_a_failure_on_the_thread_is_raised_by_the_loop(self):
        api = FakeApi()
        mix = self.mix(api)
        api.team, api.team_messages = ended_team("idle"), messages()
        ticks = iter(range(100, 10 ** 6, 10))
        mix.begin(lambda: float(next(ticks)), lambda: False, lambda: False, period=0.01)
        mix.thread.join(timeout=5)
        with self.assertRaisesRegex(AuditError, "idle"):
            mix.check()
        mix.end(raise_failure=False)

    def test_a_transient_sample_failure_is_counted_not_fatal(self):
        import subprocess
        api = FakeApi()
        mix = self.mix(api)
        api.mix_probe = mock.Mock(side_effect=subprocess.TimeoutExpired("docker", 90))
        mix.sample(400.0)
        self.assertEqual(mix.missed_samples, 1)

    def test_end_stops_the_thread(self):
        mix = self.mix(FakeApi())
        mix.begin(lambda: 0.0, lambda: True, lambda: False, period=0.01)
        mix.end()
        self.assertFalse(mix.thread.is_alive())

    def test_an_engine_command_failure_is_tried_again(self):
        import subprocess
        self.assertTrue(soak_mix.transient(subprocess.CalledProcessError(1, "docker")))
        self.assertTrue(soak_mix.transient(subprocess.TimeoutExpired("docker", 90)))
        self.assertFalse(soak_mix.transient(AuditError("leak")))


class StackWiringTests(unittest.TestCase):
    """soak_stack.Stack with and without the mix: off writes today's files; on turns teams on and mixes the script."""

    def stack(self, mix=None):
        import tempfile
        from pathlib import Path
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        installation = Path(directory.name) / "installation"
        installation.mkdir()
        (installation / "gateway.toml").write_text("[sandbox]\n")
        (installation / "models.toml").write_text(soak_stack.compose.models_toml("openai_chat"))
        arguments = {} if mix is None else {"mix": mix}
        stack = soak_stack.Stack(installation, {}, Path(directory.name) / "out", **arguments)
        stack.images = {"fake-model": "sha256:" + "e" * 64}
        with mock.patch.object(soak_stack.compose, "write", lambda path, text: path.write_text(text)):
            stack.configure(rehearsal=False)
        names = [installation / "gateway.toml", installation / "models.toml", installation / "scenario.json",
                 installation.parent / "soak.json", installation / "mix.json"]
        return {path.name: path.read_text().replace(directory.name, "<root>") for path in names if path.exists()}

    def test_off_writes_exactly_todays_files(self):
        today = self.stack()
        self.assertEqual(self.stack(soak_mix.MixOptions()), today)
        self.assertEqual(json.loads(today["scenario.json"]), scenario(False))
        self.assertNotIn("agent_teams", today["gateway.toml"])

    def test_on_turns_teams_on_and_gives_the_lanes_their_own_model(self):
        today = self.stack()
        files = self.stack(soak_mix.MixOptions(teams_every=10, desktops_every=6))
        self.assertTrue(files["gateway.toml"].endswith("[agent_teams]\nenabled = true\n"))
        self.assertEqual(files["scenario.json"], today["scenario.json"], "the slots' script is today's")
        self.assertEqual(json.loads(files["mix.json"]), soak_mix.lane_scenario(teams=True, desktops=True))
        self.assertTrue(files["models.toml"].startswith(today["models.toml"]))
        self.assertIn("name='mix'", files["models.toml"])
        overlay = json.loads(files["soak.json"])["services"]
        self.assertEqual(overlay["fake-model"], json.loads(today["soak.json"])["services"]["fake-model"])
        self.assertEqual(overlay["fake-model-mix"]["volumes"][0]["source"], "<root>/installation/mix.json")
        self.assertNotIn("mix.json", today)

    def test_desktops_alone_leave_the_switch_off(self):
        files = self.stack(soak_mix.MixOptions(desktops_every=6))
        self.assertNotIn("agent_teams", files["gateway.toml"])

    def model_check(self, turns, mix=soak_mix.MixOptions()):
        stack = soak_stack.Stack.__new__(soak_stack.Stack)
        stack.mix, stack.model = mix, "model"
        lines = "\n".join(json.dumps({"turn": turn}) for turn in turns)
        with mock.patch.object(soak_stack.Stack, "logs", lambda self, identity: lines):
            return stack.verify_model()

    def test_the_slots_model_check_is_todays_with_the_mix_too(self):
        for mix in (soak_mix.MixOptions(), soak_mix.MixOptions(teams_every=10, desktops_every=6)):
            self.assertEqual(self.model_check([0, 4], mix), {"requests": 2, "highest_response_index": 4})
            with self.assertRaises(AuditError):
                self.model_check([0, RESPONSES], mix)

    def test_a_delete_is_a_delete_with_an_idempotency_key_and_must_answer_204(self):
        stack = soak_stack.Stack.__new__(soak_stack.Stack)
        stack.origin, stack.account, stack.cookie = "http://127.0.0.1:1", None, None
        sent = []

        class Response:
            status = 204

            def __enter__(self):
                return self

            def __exit__(self, *exc):
                return False

        def open_(request, timeout):
            sent.append((request.get_method(), request.full_url, request.headers))
            return Response()
        stack.client = types.SimpleNamespace(open=open_)
        stack.delete("/api/v1/conversations/x?with_helpers=true")
        method, url, headers = sent[0]
        self.assertEqual((method, url), ("DELETE", "http://127.0.0.1:1/api/v1/conversations/x?with_helpers=true"))
        self.assertIn("Idempotency-key", headers)
        Response.status = 200
        with self.assertRaises(AuditError):
            stack.delete("/api/v1/conversations/x")


def net(a, b, c, d, prefix):
    """A network written from its numbers (the repository's guard refuses address literals)."""
    return "{}.{}.{}.{}/{}".format(a, b, c, d, prefix)


class PoolTests(unittest.TestCase):
    """The gateway wants at least twice max_live subnets in its pool; a /24 of /29s holds 32, so the mix (max_live 20)
    takes a /23 of the soak's own 10.213.0.0/16, and every other stack keeps its /24 byte for byte."""

    @staticmethod
    def todays(used, randbelow):
        """sandbox_pool as it was before the mix, verbatim."""
        import ipaddress
        taken = [ipaddress.ip_network(subnet, strict=False) for subnet in used]
        free = [candidate for candidate in (ipaddress.ip_network(net(10, 213, third, 0, 24))
                                            for third in range(256))
                if not any(candidate.version == network.version and candidate.overlaps(network) for network in taken)]
        return str(free[randbelow(len(free))])

    def test_the_default_draw_is_todays_byte_for_byte(self):
        import compose
        used = [net(10, 213, 0, 0, 24), net(10, 213, 7, 0, 25), net(172, 17, 0, 0, 16), net(10, 240, 0, 0, 16),
                "fd00::/64"]
        for draw in range(0, 300, 7):
            self.assertEqual(compose.sandbox_pool(used, randbelow=lambda n, d=draw: d % n),
                             self.todays(used, lambda n, d=draw: d % n))
        import inspect
        self.assertEqual(inspect.signature(compose.prepare).parameters["pool_prefix"].default, 24)

    def test_the_mix_takes_a_free_aligned_23(self):
        import compose
        import ipaddress
        used = [net(10, 213, 0, 0, 24), net(10, 213, 3, 0, 24)]
        for draw in range(130):
            pool = ipaddress.ip_network(compose.sandbox_pool(used, randbelow=lambda n, d=draw: d % n, prefix=23))
            self.assertEqual(pool.prefixlen, 23)
            self.assertTrue(pool.subnet_of(ipaddress.ip_network(net(10, 213, 0, 0, 16))))
            self.assertFalse(any(pool.overlaps(ipaddress.ip_network(net)) for net in used), pool)

    def test_the_mix_widens_only_with_the_mix_and_fits_the_gateways_rule(self):
        self.assertEqual(soak_mix.pool_prefix(soak_mix.MixOptions()), 24)
        self.assertEqual(soak_mix.pool_prefix(soak_mix.MixOptions(desktops_every=6)), 23)
        for slots in (4, 8):
            live = soak_mix.max_live(slots, teams=True, desktops=True)
            self.assertLessEqual(2 * live, 2 ** (29 - soak_mix.pool_prefix(soak_mix.MixOptions(teams_every=10))))
        self.assertEqual(soak_mix.max_live(8, teams=True, desktops=True), 20)

    def test_the_pool_is_read_from_the_gateways_file_for_the_evidence(self):
        cidr = net(10, 213, 6, 0, 23)
        text = '[sandbox.network_pool]\ncidr = "{}"\nprefix = 29\n'.format(cidr)
        self.assertEqual(soak_mix.network_pool(text), {"cidr": cidr, "prefix": 29})


class ProbeCadenceTests(unittest.TestCase):
    """The 2 h rehearsal of 12 Nov 2026 failed 20 s into its first Desktop task's settle: the overdue message was built
    (a full probe) on every step, outside the retry, and a desktop that stopped between its inspect and its `docker
    top` failed the run."""

    def settling(self):
        api = FakeApi()
        lane = soak_mix.DesktopLane(api, 360, start=0.0, log=lambda *a, **k: None)
        lane.step(60, paused=False, draining=False)
        api.events = [{"seq": 1, "event": {"type": "run.completed"}}]
        api.probe["screens"] = {DESK: 1}
        lane.step(70, paused=False, draining=False)
        self.assertEqual(lane.state, "settling")
        api.mix_probe = mock.Mock(wraps=api.mix_probe)
        return lane, api

    def test_a_step_between_polls_never_probes(self):
        lane, api = self.settling()
        for now in (71, 72, 73, 74):
            lane.step(now, paused=False, draining=False)
        api.mix_probe.assert_not_called()
        lane.step(75, paused=False, draining=False)
        self.assertEqual(api.mix_probe.call_count, 1)

    def test_a_probe_failure_while_settling_is_tried_again(self):
        import subprocess
        lane, api = self.settling()
        api.mix_probe.side_effect = subprocess.CalledProcessError(1, ["docker", "top"])
        lane.step(75, paused=False, draining=False)
        self.assertEqual(lane.retries, 1)
        api.mix_probe.side_effect = None
        api.probe["screens"] = {}
        lane.step(80, paused=False, draining=False)
        self.assertTrue(lane.idle())

    def test_a_desktop_that_stops_before_its_top_has_no_processes(self):
        import subprocess
        record = {"Id": "c2", "Image": DESKTOP_IMAGE, "State": {"Status": "running"},
                  "Config": {"Labels": {"tqa.conversation": DESK}}, "HostConfig": {"Tmpfs": {}}}
        stack = types.SimpleNamespace(env={"TQA_SANDBOX_NETWORK": "net", "TQA_STORAGE_ROOT": "/s"}, database="db",
                                      inspect=lambda identity: dict(record, State={"Status": "running"}))

        def command(arguments, timeout=90):
            if arguments[:2] == ["docker", "ps"]:
                return "c2"
            if arguments[:2] == ["docker", "top"]:
                raise subprocess.CalledProcessError(1, arguments, stderr="Error response from daemon: container c2 "
                                                                           "is not running")
            return "teams|0\nteam_log_rows|0\nworkspace_copies|0\npending_team_turns|0\nreservations|0\n"
        stack.command, stack.helper = command, lambda arguments: ""
        result = mix_audit.MixProbe(stack, DESKTOP_IMAGE, toybox="toybox")(set())
        self.assertEqual(result["counts"]["desktop_processes"], 0)

    def test_another_top_failure_still_fails_the_probe(self):
        import subprocess
        stack = types.SimpleNamespace(env={"TQA_SANDBOX_NETWORK": "net", "TQA_STORAGE_ROOT": "/s"}, database="db",
                                      inspect=lambda identity: {"Id": "c2", "Image": DESKTOP_IMAGE,
                                                                "State": {"Status": "running"},
                                                                "Config": {"Labels": {}}, "HostConfig": {}})

        def command(arguments, timeout=90):
            if arguments[:2] == ["docker", "ps"]:
                return "c2"
            raise subprocess.CalledProcessError(1, arguments, stderr="Error response from daemon: permission denied")
        stack.command = command
        with self.assertRaises(subprocess.CalledProcessError):
            mix_audit.MixProbe(stack, DESKTOP_IMAGE, toybox="toybox")(set())


if __name__ == "__main__":
    unittest.main()
