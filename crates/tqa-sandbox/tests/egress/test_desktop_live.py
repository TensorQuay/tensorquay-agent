"""The desktop stack cases' fixture, without Docker: the scripted runs, their order, the pages and the parsers."""
import struct
import unittest
import zlib

from egress import desktop_kept, desktop_live

FIELDS = {"action", "ref", "text", "keys", "ms", "direction", "amount"}


def tool_calls(case):
    return [event for response in desktop_live.scenario(case, "192.0.2.10")["responses"]
            for event in response["events"] if event.get("type") == "tool_call"]


class ScenarioTests(unittest.TestCase):
    def test_each_conversation_has_one_turn_per_call_and_one_closing_text_per_run(self):
        table = desktop_live.runs("192.0.2.10")
        for case, conversations in desktop_live.CONVERSATIONS.items():
            responses = desktop_live.scenario(case, "192.0.2.10")["responses"]
            for phrase, names in conversations:
                own = [response for response in responses if response["when"] == phrase]
                self.assertEqual(len(own), sum(len(table[name]) + 1 for name in names), (case, phrase))
                texts = [response["events"][0]["text"] for response in own if response["events"][0]["type"] == "text"]
                self.assertEqual(texts, ["run {} ({}) done".format(index + 1, name)
                                         for index, name in enumerate(names)])

    def test_every_reply_is_chosen_by_its_conversations_phrase(self):
        # By turn alone a new task started over at the first run's calls: the capped task got the act run's.
        for case in desktop_live.CONVERSATIONS:
            found = desktop_live.scenario(case, "192.0.2.10")
            self.assertEqual(found["select"], "prompt")
            phrases = [phrase for phrase, _ in desktop_live.CONVERSATIONS[case]]
            self.assertEqual({response["when"] for response in found["responses"]}, set(phrases))
            # The model matches a phrase anywhere in the first message: no phrase may hold another.
            self.assertFalse([(a, b) for a in phrases for b in phrases if a != b and a in b], case)

    def test_the_conversations_hold_the_runs_the_driver_sends(self):
        for case, order in desktop_live.ORDER.items():
            held = {name for _, names in desktop_live.CONVERSATIONS[case] for name in names}
            self.assertEqual(held, set(order), case)

    def test_each_phrase_is_in_its_conversations_first_message(self):
        spec = (desktop_live.ROOT / "web" / "e2e" / "screen-stack.spec.ts").read_text()
        for case, conversations in desktop_live.CONVERSATIONS.items():
            for phrase, names in conversations:
                if case == "sign-in" and names[0] == "watched":
                    # The browser sends the watched task's first message.
                    self.assertIn('input.fill("{}")'.format(phrase), spec)
                else:
                    # The driver's own first message for a run (LiveStack.run).
                    self.assertIn(phrase, "Do the {} steps.".format(names[0]))

    def test_every_computer_call_carries_the_strict_schemas_fields(self):
        for case in desktop_live.ORDER:
            for call in tool_calls(case):
                if call["name"] == "computer":
                    self.assertEqual(set(call["arguments"]), FIELDS)

    def test_the_checked_steps_sit_where_the_checks_read_them(self):
        table = desktop_live.runs("192.0.2.10")
        act = [arguments for _, arguments in table["act"]]
        self.assertEqual([act[4]["action"], act[5]["ref"], act[6]["ref"], act[7]["action"], act[8]["ref"],
                          act[9]["ref"]], ["screenshot", "e1", "2", "observe", "e9", "1"])
        self.assertEqual(table["decoy"][5][1]["ref"], "7")
        self.assertEqual(table["detail"][4][1]["action"], "screenshot")
        clip = [arguments.get("keys") or arguments.get("action") for _, arguments in table["clipboard"]]
        self.assertEqual(clip[4:], ["observe", "click", "ctrl+v", "observe", "ctrl+a", "ctrl+c", "wait"])
        # The caps case's four navigation actions spend the cap of four before its key.
        self.assertEqual([arguments["action"] for _, arguments in table["caps"]],
                         ["key", "type", "key", "wait", "screenshot", "screenshot", "key"])
        watched = [arguments for _, arguments in table["watched"]]
        self.assertEqual([watched[4]["action"], watched[5]["ref"], watched[7]["ref"]], ["screenshot", "2", "e9"])
        self.assertEqual(table["whoami"][1][1]["text"], "http://192.0.2.10:8081/whoami")
        self.assertEqual(table["whoami"][4][1]["action"], "observe")
        dialog = [arguments.get("ref") or arguments.get("keys") or arguments["action"]
                  for _, arguments in table["dialog"]]
        self.assertEqual(dialog[4:], ["observe", "e1", "Return", "observe"])

    def test_the_sign_in_case_runs_whoami_after_the_watched_run(self):
        self.assertEqual(desktop_live.ORDER["sign-in"], ["watched"] + ["whoami"] * 6)

    def test_the_kept_case_runs_nine_times_over_two_tasks_on_one_script(self):
        """WEB-E2E-125: six runs on the first task (whoami before and after the sign-in and after the pause, the run
        that kills the browser, whoami after the restart and after the switch went off) and three on the second
        (before, after the sign-in, after Sign out everywhere), both tasks on the one six-run script their shared
        phrase picks: the second task's three whoami runs are the script's first three."""
        self.assertEqual(desktop_live.ORDER["kept"], ["whoami"] * 3 + ["kill"] + ["whoami"] * 5)
        self.assertEqual(desktop_live.CONVERSATIONS["kept"],
                         [("Do the whoami steps", ["whoami"] * 3 + ["kill"] + ["whoami"] * 2)])
        self.assertEqual(desktop_live.DEADLINE_SECONDS["kept"], 1800)
        # The idle limit is short, and the pause is three of them; the cookie flush wait covers Chromium's batch.
        self.assertEqual(desktop_kept.KEPT_IDLE_SECONDS, 20)
        self.assertGreater(desktop_kept.COOKIE_FLUSH_SECONDS, 30)

    def test_the_kill_run_ends_the_browser_inside_the_run_and_says_so(self):
        """The browser dies while a run holds the desktop (the runtime sees no Take over, so a death while idle is only
        found at the next run's start or reset): one shell call, by the browser's command line, and its word."""
        table = desktop_live.runs("192.0.2.10")
        self.assertEqual(table["kill"], [("shell", {"command": desktop_live.KILL_BROWSER})])
        self.assertIn("[/]usr/lib/chromium/chromium", desktop_live.KILL_BROWSER)
        self.assertTrue(desktop_live.KILL_BROWSER.endswith("echo killed"))

    def test_the_kept_ready_phase_reads_the_control_line_the_reset_settles(self):
        """After the kill run, the driver's Take over must be granted ready (the kept reset started the browser again
        and said so): the phase waits for the plain control line, never the not-ready one, then hands back."""
        spec = (desktop_live.ROOT / "web" / "e2e" / "screen-stack.spec.ts").read_text()
        self.assertIn('PHASE !== "kept-ready"', spec)
        phase = spec[spec.index('PHASE !== "kept-ready"'):]
        phase = phase[:phase.index("await result(checked)")]
        self.assertIn('words("screen.control")', phase)
        self.assertNotIn("screen.controlNotReady", phase)
        self.assertIn('"Hand back"', phase)

    def test_the_browser_cookie_probe_takes_several_names(self):
        self.assertIn("process.argv[1].split(',')", desktop_live.BROWSER_COOKIES)


class PageTests(unittest.TestCase):
    def test_the_marks_page_has_nine_buttons_in_order_and_one_painted_seven(self):
        page = desktop_live.marks_page()
        self.assertEqual([page.index(">Button {}<".format(letter)) for letter in desktop_live.BUTTONS],
                         sorted(page.index(">Button {}<".format(letter)) for letter in desktop_live.BUTTONS))
        self.assertEqual(page.count(">7</span>"), 1)

    def test_the_dialog_page_names_its_answer_in_the_title(self):
        page = desktop_live.dialog_page()
        self.assertIn("confirm('Sure?')?'confirmed':'declined'", page)
        self.assertEqual(page.count("<button"), 1, "its one button is the first ref, e1")

    def test_the_form_saves_what_was_typed_into_the_title(self):
        self.assertIn("'saved '+document.querySelector('input').value", desktop_live.form_page())
        self.assertIn("aria-label='Name'", desktop_live.form_page())

    def test_the_noise_page_fills_more_than_the_smallest_screenshot(self):
        self.assertIn("width=1920 height=1200", desktop_live.noise_page())

    def test_the_rfb_probe_compiles(self):
        compile(desktop_live.RFB_PROBE, "rfb_probe.py", "exec")


class ParserTests(unittest.TestCase):
    def test_listing_reads_refs_roles_and_names(self):
        text = 'Screenshot 1280×800\n  e1 textbox "Name"\n  e2 button "Save"\n'
        self.assertEqual(desktop_live.listing(text), {"e1": ("textbox", "Name"), "e2": ("button", "Save")})

    def test_png_size_reads_the_header_and_refuses_anything_else(self):
        header = struct.pack(">II", 640, 400) + bytes([8, 2, 0, 0, 0])
        png = (b"\x89PNG\r\n\x1a\n" + struct.pack(">I", 13) + b"IHDR" + header
               + struct.pack(">I", zlib.crc32(b"IHDR" + header)))
        self.assertEqual(desktop_live.png_size(png), (640, 400))
        with self.assertRaises(AssertionError):
            desktop_live.png_size(b"GIF89a" + bytes(30))

    def test_calls_pairs_starts_results_and_captures(self):
        def item(seq, event):
            return {"seq": seq, "event": event}
        capture = {"type": "screen.capture", "call_id": "c1", "phase": "before", "id": "s1"}
        events = [item(1, capture), item(2, {"type": "tool.start", "call_id": "c1"}),
                  item(3, {"type": "tool.result", "call_id": "c1", "status": "ok", "content": ""})]
        [step] = desktop_live.calls(events)
        self.assertEqual((step["start"], step["end"], step["captures"]["before"]["seq"]), (2, 3, 1))


INIT = "/bin/sh /usr/local/bin/tqa-desktop-init"


def table(runtime_parent, loops_parent):
    """A Desktop sandbox's processes as PROCESSES writes them: tini, desktop-init, the runtime, and one keep loop per
    part with the part under it."""
    lines = ["1 0 /usr/bin/tini -- /usr/local/bin/tqa-desktop-init", "7 1 " + INIT,
             "9 {} /usr/local/bin/tqa-runtime".format(runtime_parent)]
    for index, part in enumerate(desktop_live.PARTS):
        loop, pid = 20 + 2 * index, 21 + 2 * index
        lines += ["{} {} {}".format(loop, loops_parent, INIT), "{} {} {} --flag".format(pid, loop, part)]
    return "\n".join(lines) + "\n"


# TEST: TK-U-051
class ReapTests(unittest.TestCase):
    def test_a_stack_beside_the_runtime_outlives_it(self):
        self.assertEqual(desktop_live.outlived(desktop_live.process_table(table(7, 7))), [])

    def test_a_stack_under_the_runtime_is_named_part_by_part(self):
        # A shape before 17.0: desktop-init became the runtime (pid 7), so its loops are the runtime's children.
        text = table(7, 9)
        problems = desktop_live.outlived(desktop_live.process_table(text))
        self.assertEqual(len(problems), len(desktop_live.PARTS))
        self.assertTrue(all("descends from the runtime (9)" in problem for problem in problems), problems)

    def test_parts_gone_and_no_runtime_are_named(self):
        # What the reap leaves: tini and the runtime only, then a sandbox whose runtime has ended too.
        reaped = "1 0 /usr/bin/tini -- /usr/local/bin/tqa-desktop-init\n9 1 /usr/local/bin/tqa-runtime\n"
        self.assertEqual(desktop_live.outlived(desktop_live.process_table(reaped)),
                         ["{} isn't running".format(part) for part in desktop_live.PARTS])
        self.assertIn("the runtime isn't running",
                      desktop_live.outlived(desktop_live.process_table(reaped.splitlines()[0])))

    def test_the_process_table_skips_kernel_threads_and_odd_lines(self):
        parsed = desktop_live.process_table("12 1 /usr/bin/Xvfb :0\nnot a line\n13 x y\n14 2 \n")
        self.assertEqual(parsed, {12: (1, "/usr/bin/Xvfb :0"), 14: (2, "")})

    def test_the_reap_run_opens_an_application_then_looks_then_reads_the_screen(self):
        run = desktop_live.runs("192.0.2.10")["reap"]
        self.assertEqual([name for name, _ in run], ["shell", "shell", "computer"])
        # Mousepad reaches the display only after its call has returned, and the check waits past the 10 s grace.
        self.assertIn("setsid sh -c 'sleep 3; exec mousepad'", run[0][1]["command"])
        self.assertNotIn("; sleep 2", run[0][1]["command"])
        self.assertIn("sleep 300", run[0][1]["command"])
        self.assertTrue(run[1][1]["command"].startswith("sleep 12;"))
        self.assertEqual(desktop_live.ORDER["reap"], ["reap"])

    def test_survivors_reads_which_processes_outlived_the_reap(self):
        text = "mousepad alive\nsleep gone\nchromium alive\nnoise here and there\n"
        self.assertEqual(desktop_live.survivors(text), {"mousepad": True, "sleep": False, "chromium": True})
        self.assertEqual(desktop_live.survivors(""), {})


class FixtureSiteLoginsTests(unittest.TestCase):
    """SEC-059 (16r's sec_059_a): the fixture site says how many times its login page was served, so the driver tells a
    sign-in by hand that never reached the site from one whose cookie went missing."""

    def test_logins_counts_each_login_page_served(self):
        import threading
        import urllib.request
        from egress import upstream
        server = upstream.Server(("127.0.0.1", 0), upstream.Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.shutdown)
        base = "http://127.0.0.1:{}".format(server.server_address[1])

        def logins():
            page = urllib.request.urlopen(base + "/logins", timeout=5).read().decode()
            return int(page.split("<title>")[1].split("</title>")[0])

        start = logins()
        urllib.request.urlopen(base + "/whoami", timeout=5).read()
        self.assertEqual(logins(), start, "only the login page counts")
        for served in (1, 2):
            urllib.request.urlopen(base + "/login", timeout=5).read()
            self.assertEqual(logins(), start + served)

    def test_login_sets_a_persistent_and_a_session_cookie_and_whoami_says_which_came(self):
        """WEB-E2E-125: the persistent cookie decides signed in or out; the session cookie's fate is said after it."""
        import threading
        import urllib.request
        from egress import upstream
        server = upstream.Server(("127.0.0.1", 0), upstream.Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.shutdown)
        base = "http://127.0.0.1:{}".format(server.server_address[1])
        with urllib.request.urlopen(base + "/login", timeout=5) as response:
            cookies = response.headers.get_all("Set-Cookie")
        self.assertEqual(len(cookies), 2, cookies)
        persistent = next(cookie for cookie in cookies if cookie.startswith(upstream.SESSION + "="))
        session = next(cookie for cookie in cookies if cookie.startswith(upstream.TAB + "="))
        self.assertIn("Max-Age=3600", persistent)
        self.assertNotIn("Max-Age", session)
        self.assertNotIn("Expires", session)

        def title(cookie_header):
            request = urllib.request.Request(base + "/whoami", headers={"Cookie": cookie_header} if cookie_header else {})
            page = urllib.request.urlopen(request, timeout=5).read().decode()
            return page.split("<title>")[1].split("</title>")[0]

        both = "{}=signed-in; {}=signed-in".format(upstream.SESSION, upstream.TAB)
        self.assertEqual(title(both), upstream.SIGNED_IN + "; " + upstream.TAB_KEPT)
        self.assertEqual(title(upstream.SESSION + "=signed-in"), upstream.SIGNED_IN + "; " + upstream.TAB_GONE)
        self.assertEqual(title(upstream.TAB + "=signed-in"), upstream.SIGNED_OUT + "; " + upstream.TAB_KEPT)
        self.assertEqual(title(None), upstream.SIGNED_OUT + "; " + upstream.TAB_GONE)
        # The driver's readers tell the two apart by their own words, neither a part of the other.
        for a, b in ((upstream.SIGNED_IN, upstream.SIGNED_OUT), (upstream.TAB_KEPT, upstream.TAB_GONE)):
            self.assertNotIn(a, b)
            self.assertNotIn(b, a)


if __name__ == "__main__":
    unittest.main()
