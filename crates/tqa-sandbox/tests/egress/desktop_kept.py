"""WEB-E2E-125 live: a desktop sign-in kept for the whole task (ADR-0039 §6 as amended for 18.0; PRD item 24), the
`kept` case of `egress.desktop_live`, on its live desktop stack: the fixture site's two cookies (upstream.py), the runs
and the checks of desktop_live, and screen-stack.spec.ts's kept-switch and kept-state phases for the Screen panel."""

import time

from .desktop_live import calls, hand_sign_in, whoami
from .upstream import SESSION, TAB, TAB_GONE, TAB_KEPT

# The case's idle limit: short, so a pause longer than it fits the case; set before the task's desktop starts (a
# sandbox keeps the idle limit it started with), and three of them are waited.
KEPT_IDLE_SECONDS = 20


def tab_cookie(events):
    """Whether the site's session cookie came with the run's request too (the browser's memory, not the profile)."""
    content = calls(events)[4]["result"]["content"]
    if TAB_KEPT in content:
        return True
    assert TAB_GONE in content, "the site's page wasn't read: " + content[:400]
    return False


# The runtime's browser, by its command line (REAP_OPEN's pattern), ended as a crash would end it.
KILL_BROWSER = "pkill -f '[/]usr/lib/chromium/chromium .*--user-data-dir'; true"
# Chromium writes its cookie store in batches, about every 30 s: the persistent cookie must be on disk before the browser
# is killed, or the kill takes it with the memory and the case proves nothing about the reset.
COOKIE_FLUSH_SECONDS = 45


def kept(stack, directory):
    """WEB-E2E-125 live, two tasks. The first: signed out, the switch turned on in the browser (the panel's words), a
    sign-in by hand, then the run that follows is signed in with both cookies; a pause of three idle windows leaves the
    desktop up and the next run signed in; the browser killed and started again by the next run, that run has the
    persistent cookie and not the session one; the switch turned off empties the profile at once and the next run is
    signed out. The second: the switch on, a sign-in, Sign out everywhere, the next run signed out, the switch still on."""
    login = "http://{}:8081/login".format(stack.upstream)
    stack.limit("admission.slots", max(2, next(item["value"] for item in stack.request("/api/v1/admin/limits")["items"]
                                                if item["key"] == "admission.slots")))
    stack.limit("desktop.max_sessions", 2)
    stack.limit("sandbox.idle_seconds", KEPT_IDLE_SECONDS)
    task = stack.task("Desktop sign-in kept")["id"]
    assert whoami(stack.run(task, "whoami")) == "out", "a new desktop was signed in"
    found = stack.browser("kept-switch", directory / "kept-on.json", TQA_SCREEN_TASK=task, TQA_SCREEN_KEPT_TO="on")
    assert found.get("checked") == ["was-off", "turned-on", "ends-line", "notice"], found
    held = hand_sign_in(stack, directory / "kept-sign-in.json", task, login)
    assert set(stack.browser_cookies(task, (SESSION, TAB))) == {(SESSION, stack.upstream), (TAB, stack.upstream)}, \
        "the sign-in by hand set both of the site's cookies"
    events = stack.run(task, "whoami")
    assert whoami(events) == "in", "the run after the sign-in wasn't signed in with the switch on"
    assert tab_cookie(events), "the kept reset lost the session cookie: the browser wasn't kept alive"
    assert stack.browser_cookies(task) == held, "the run's end changed the browser's cookies with the switch on"
    print("WEB-E2E-125 live: with the switch on, the sign-in by hand served the run after it, both cookies, and the "
          "run's end kept the browser and its profile", flush=True)
    sandbox = stack.sandbox_of(task)
    time.sleep(3 * KEPT_IDLE_SECONDS)
    assert stack.sandbox_of(task) == sandbox, "the kept desktop was retired or replaced during the pause"
    assert stack.browser_cookies(task) == held, "the pause changed the browser's cookies"
    events = stack.run(task, "whoami")
    assert whoami(events) == "in" and tab_cookie(events), "the run after the pause wasn't signed in with both cookies"
    print("WEB-E2E-125 live: a pause of {} s, three idle windows, retired nothing and reset nothing: the next run was "
          "signed in".format(3 * KEPT_IDLE_SECONDS), flush=True)
    # The browser dies between runs: the next run's start brings a new one, whose memory holds no session cookie, while
    # the persistent cookie is in the profile on disk (written by now: Chromium's batch, and its own clean exit).
    time.sleep(COOKIE_FLUSH_SECONDS)
    stack.in_sandbox(task, ["sh", "-c", KILL_BROWSER])
    events = stack.run(task, "whoami")
    assert whoami(events) == "in", "the persistent cookie didn't survive the browser's restart"
    assert not tab_cookie(events), "the session cookie survived a browser started again"
    assert stack.browser_cookies(task, (SESSION, TAB)) == [(SESSION, stack.upstream)], \
        "the started-again browser holds the persistent cookie and only it"
    print("WEB-E2E-125 live: the browser killed and started again, the next run had the persistent cookie and not the "
          "session cookie: a kept sign-in is the profile's, and a session cookie lives only while the browser does",
          flush=True)
    found = stack.browser("kept-switch", directory / "kept-off.json", TQA_SCREEN_TASK=task, TQA_SCREEN_KEPT_TO="off")
    assert found.get("checked") == ["was-on", "turned-off", "no-ends-line", "notice"], found
    assert stack.browser_cookies(task, (SESSION, TAB)) == [], "the switch turned off left the cookies in the browser"
    assert stack.cookies(task) == [], "the switch turned off left the cookie on disk: {}".format(stack.cookies(task))
    assert whoami(stack.run(task, "whoami")) == "out", "the run after the switch went off was signed in"
    print("WEB-E2E-125 live: the switch turned off emptied the profile at once, and the next run was signed out",
          flush=True)

    second = stack.task("Desktop sign-in kept, then cleared", files=False)["id"]
    assert whoami(stack.run(second, "whoami")) == "out", "the second task's desktop was signed in"
    found = stack.browser("kept-switch", directory / "kept-on-2.json", TQA_SCREEN_TASK=second, TQA_SCREEN_KEPT_TO="on")
    assert found.get("checked") == ["was-off", "turned-on", "ends-line", "notice"], found
    hand_sign_in(stack, directory / "kept-sign-in-2.json", second, login)
    assert whoami(stack.run(second, "whoami")) == "in", "the second task's run after the sign-in wasn't signed in"
    found = stack.browser("sign-out", directory / "kept-sign-out.json", TQA_SCREEN_TASK=second)
    assert found.get("checked") == ["signed-out"], found
    assert stack.browser_cookies(second, (SESSION, TAB)) == [], "Sign out everywhere left a cookie in the browser"
    assert stack.cookies(second) == [], "Sign out everywhere left the cookie: {}".format(stack.cookies(second))
    found = stack.browser("kept-state", directory / "kept-state.json", TQA_SCREEN_TASK=second, TQA_SCREEN_KEPT_IS="on")
    assert found.get("checked") == ["is-on", "ends-line"], found
    assert whoami(stack.run(second, "whoami")) == "out", "the run after Sign out everywhere was signed in"
    print("WEB-E2E-125 live: Sign out everywhere emptied a kept profile at once, left the switch on, and the next run "
          "was signed out", flush=True)
