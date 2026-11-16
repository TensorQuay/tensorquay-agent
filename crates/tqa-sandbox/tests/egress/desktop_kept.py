"""WEB-E2E-125 live: a desktop sign-in kept for the whole task (ADR-0039 §6 as amended for 18.0; PRD item 24), the
`kept` case of `egress.desktop_live`, on its live desktop stack: the fixture site's two cookies (upstream.py), the runs
and the checks of desktop_live, and screen-stack.spec.ts's kept-switch and kept-state phases for the Screen panel."""

import re
import time

from .desktop_live import KILL_BROWSER, calls, hand_sign_in, whoami
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


# Chromium writes its cookie store in batches, about every 30 s: the persistent cookie must be on disk before the browser
# is killed (KILL_BROWSER, the kill run), or the kill takes it with the memory and the case proves nothing about the
# restart.
COOKIE_FLUSH_SECONDS = 45


def exit_type(content):
    """The dead browser's exit mark, read by the kill run's call from the profile's Preferences (KILL_BROWSER):
    "Normal" after a clean exit, "SessionEnded" when a signal ended it in order (run 7 of 16 Nov 2026, no session
    cookie restored), "Crashed" when the mark Chromium sets at start was never cleared; unknown otherwise."""
    found = re.search(r'"exit_type":"([A-Za-z]+)"', content)
    return found.group(1) if found else "unknown"


def restarted(held, upstream, mark):
    """The kill check's verdict on the cookies the started-again browser holds: the persistent cookie must be there (a
    kept sign-in is the profile's); the session cookie may be or not, since Chromium restores session cookies when its
    last exit was a crash and a kill may count as one (run 6 of 16 Nov 2026 found it back, runs 4 and 6b didn't), so
    it is recorded with the exit mark, never failed on. Returns the words for the printed line."""
    assert (SESSION, upstream) in held, "the started-again browser doesn't hold the persistent cookie: {}".format(held)
    assert all(name in (SESSION, TAB) and domain == upstream for name, domain in held), \
        "the started-again browser holds a cookie that isn't the site's two: {}".format(held)
    cookie = "with the session cookie restored" if (TAB, upstream) in held else "without the session cookie"
    return "{} (the dead browser's exit mark: {})".format(cookie, mark)


def kept(stack, directory):
    """WEB-E2E-125 live, two tasks. The first: signed out, the switch turned on in the browser (the panel's words), a
    sign-in by hand, then the run that follows is signed in with both cookies; a pause of three idle windows leaves the
    desktop up and the next run signed in; the browser killed inside a run and started again by that run's end reset,
    which says it is ready (a Take over is granted plain), the persistent cookie still in the profile and the new
    browser, and the next run's keys land in the new window: it has the persistent cookie and not the session one; the
    switch turned off empties the profile at once and the next run is signed out. The second: the switch on, a sign-in,
    Sign out everywhere, the next run signed out, the switch still on."""
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
    # The browser dies inside a run (the runtime sees no Take over, so a death while idle is found only at the next
    # run's start or reset): that run's end reset finds it dead and starts it again (RT-I-061), whose memory holds no
    # session cookie, while the persistent cookie is in the profile on disk (written by now: Chromium's batch, and its
    # own clean exit). Dead Chromium helpers may still hold the profile's lock, so the relaunch is proved three ways:
    # the reset said ready (the Take over right after it is granted plain, not "may not last"), the new browser answers
    # with the persistent cookie, and the next run's keys reach the site through the new window.
    time.sleep(COOKIE_FLUSH_SECONDS)
    events = stack.run(task, "kill")
    content = calls(events)[0]["result"]["content"]
    assert "killed" in content, "the kill run's shell call didn't run: {}".format(calls(events)[0]["result"])
    found = stack.browser("kept-ready", directory / "kept-ready.json", TQA_SCREEN_TASK=task)
    assert found.get("checked") == ["control-ready", "handed-back"], found
    assert stack.browser_up(task), "no browser on the desktop after the kill run's end reset"
    verdict = restarted(stack.browser_cookies(task, (SESSION, TAB)), stack.upstream, exit_type(content))
    written = stack.cookies(task)
    assert written and all(path.startswith("/run/tqa-browser/") for path in written), \
        "the persistent cookie is in the runtime's profile and nowhere else: {}".format(written)
    events = stack.run(task, "whoami")
    assert whoami(events) == "in", "the run after the restart wasn't signed in: its keys didn't reach the new window"
    print("WEB-E2E-125 live: the browser killed inside a run, its end reset started it again and said ready, the "
          "profile's cookie was still there, and the next run's keys landed in the new window with the persistent "
          "cookie: a kept sign-in is the profile's. The new browser started {}; the next run's request {} the session "
          "cookie".format(verdict, "carried" if tab_cookie(events) else "didn't carry"), flush=True)
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
