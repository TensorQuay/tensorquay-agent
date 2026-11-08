# Gate 11: Computer use

**State:** **draft, not reached** (21 Oct 2026, for the technical lead). The Evaluation below is the technical lead's
to write. The release candidate is 16y. In the 16y integration (20 Oct), every row in this gate's
scope passed, among them the sandbox's Desktop tests (SEC-059's sign-in by hand included), the Desktop suite's 7 tests
and the 18 rows added on 21 Oct. No computer-use test has failed in an integration since 16u.

After 16j, a real-world campaign on 16k (17 Oct) gave Desktop tasks goals a person would set. The defects it found
were fixed in 16m to 16p and checked again live on the real model on 16o and 16x (below).

**The primary evidence for 16y's computer use:**
- the 16y integration;
- 16k's real-world Desktop cases;
- the live checks on 16o (the campaign's fixes), on 16u (L1: 16s's reset-ready fix) and on 16x (G1, L4, L1, W8, V1 and
  W9: the zbus-6 reader, the sign-out readiness fix, and the campaign's fixes again as regressions).

16y itself is 16x plus TCP keepalive on the gateway's accepted sockets (GW-U-205, GW-I-314). It applies to every
accepted socket, the Screen panel's viewer and the Desktop sandboxes' connections among them, but it changes behaviour
only for an idle connection whose peer is gone, which no computer-use test or live check relies on. The phase demo on
16e (13 Oct), with its last step on 16i (14 Oct), is supporting evidence.

What isn't done yet:
- **a soak with Desktop tasks in its mix: open.** No c16 soak has had one (16e, 16j, 16u, 16x and 16y's). This
  doesn't block 16y, but it isn't closed;
- the grounding evaluation on 16y's own capture (below): the targets stand, but the tool's words and the Desktop's
  root window changed after 16i;
- the Evaluation below.

## Scope
The TEST-PLAN §17 Gate 11 row (the technical lead, 12 Oct 2026; the rows added since, 21 Oct) holds every P0 and P1
row that names Phase 11 or ADR-0039, plus WEB-E2E-067; the phase demo; and the grounding evaluation as the gate's
measurement. Retired rows (TOOL-U-046, WEB-E2E-076, WEB-U-240) are left out.
- **P0 (22):** GW-I-159, GW-I-218, GW-I-223, GW-I-313; GW-U-147, GW-U-150, GW-U-151, GW-U-203, GW-U-204; RT-I-054…057;
  SBX-C-020 (on Docker, and the microVM's refusal in v1.1), SBX-C-022; SEC-001 (repeated in a Desktop sandbox), SEC-005
  (the screen socket), SEC-057, SEC-058, SEC-059; TOOL-U-057, TOOL-U-062.
- **P1 (54):** CORE-U-081…083; GW-I-217, GW-I-219…222, GW-I-224, GW-I-225, GW-I-228, GW-I-230, GW-I-274, GW-I-305,
  GW-I-308; GW-U-146, GW-U-148, GW-U-152, GW-U-155, GW-U-196, GW-U-200; LLM-U-052; PROTO-U-036…041; RT-I-045…049,
  RT-I-051, RT-I-053; TOOL-U-043…045, TOOL-U-047, TOOL-U-048, TOOL-U-050, TOOL-U-059, TOOL-U-061, TOOL-U-063,
  TOOL-U-067, TOOL-U-068; WEB-E2E-067
  (Settings › Limits › Desktop on the real gateway), WEB-E2E-071…074; WEB-U-306, WEB-U-329, WEB-U-377.
- **The phase demo** (DEV-PLAN Phase 11, ARCHITECTURE §4.7): a Desktop task watched, taken over and handed back.
- **The measurement:** the grounding evaluation (`computer-grounding.md`). It doesn't pass or fail the gate. It
  decides which routes get `pixel_grounding` (ADR-0039 §2), and its result is under Known limits below.

**The rows added on 21 Oct** (the technical lead) are GW-I-274, already in the §17 row, and the computer-use P0 and P1
rows written since 12 Oct: those naming ADR-0039, and those this page cites as a fix. Each passed in the 16y
integration, by the test named:
- a viewer waits for a live sandbox whose runtime link is down: GW-I-274
  (`gw_i_274_a_viewer_waits_for_a_live_sandbox_whose_link_is_down`; WEB-E2E-073's flake);
- Sign out everywhere answers once the browser is back, with its `ready` (P0): RT-I-056
  (`rt_i_056_sign_out_everywhere_answers_after_the_browser_is_back_with_its_ready`), RT-I-057 (a reopened browser is
  ready only with the X input focus and its page answering:
  `rt_i_057_the_focus_is_the_input_focus_not_only_the_active_window` and three more), GW-U-204
  (`gw_u_204_a_take_over_around_sign_out_everywhere_waits_and_is_not_plain_without_a_browser`);
- a finished task's desktop gives way to a waiting desktop task, unless a sign-in may be in it (16k's A4 and A6):
  GW-I-305 (`gw_i_305_a_finished_desktop_gives_way_to_a_waiting_one` and
  `gw_i_305_a_desktop_taken_over_since_its_reset_keeps_its_sign_in`), GW-I-308
  (`gw_i_308_a_desktop_that_may_hold_a_sign_in_isnt_evicted_for_a_slot`), GW-U-200
  (`gw_u_200_a_desktop_gives_way_only_with_its_profile_known_empty`);
- a run's end reset says ready only once the browser is up and focused (16r's SEC-059; P0): RT-I-055
  (`rt_i_055_a_reset_says_ready_only_when_the_browser_is_up_and_focused_in_time`), GW-U-203
  (`gw_u_203_a_reset_without_a_ready_browser_keeps_control_not_ready`), GW-I-313
  (`gw_i_313_a_reset_without_a_ready_browser_grants_control_marked_not_ready`);
- a page's file chooser through the computer tool (16k's A12b; P0): TOOL-U-062
  (`tool_u_062_a_click_by_ref_is_a_persons_gesture_so_a_chooser_may_open` and six more);
- typing into a date or time field (16k's A2): TOOL-U-059 (`tool_u_059_a_date_or_time_segment_is_typed_as_keys` and six
  more); how to open a web address (A5, A7): TOOL-U-061 (`tool_u_061_the_tool_says_how_to_open_a_web_address`); a
  browser window that isn't its page (A12b): TOOL-U-063
  (`tool_u_063_a_browser_window_that_isnt_the_page_is_an_application`);
- a run that ends while nobody watches holds the next Take over until its reset (16f's SEC-059): GW-U-196
  (`gw_u_196_a_run_ended_while_nobody_watched_holds_control_until_its_reset`);
- the accessibility reader on zbus 6: TOOL-U-067 (`atspi_wire_tests`, e.g. `tool_u_067_the_state_names_are_pinned`)
  and TOOL-U-068 (`atspi_tree_tests`, e.g. `tool_u_068_a_bus_that_isnt_there_is_unavailable`);
- the screen client gets the server's first frames after it has attached: WEB-U-377 (`src/screen/screen.test.tsx`,
  32 tests passed).

**Other evidence, not criteria** (P2, as for the other gates): WEB-E2E-037, WEB-E2E-075, WEB-E2E-077, WEB-E2E-082;
WEB-U-241…248, WEB-U-259, WEB-U-261, WEB-U-268, WEB-U-279, WEB-U-280, WEB-U-282. TOOL-U-034 (P0) is evidence, not a
criterion: it is every strict tool's rule for null optional arguments, and only one of its 13 tests is the computer
tool's (16k's null-word ref). ADR-0039's security review maps each threat T1…T15 to a test; the P0 rows above carry
them.

## The 16y integration (20 Oct 2026, its summary from 20:08 to 23:16 +08:00)
**Source:** the integration's summary `int-5a59ad1d5-summary.log`, with each step's log in `int-5a59ad1d5-*.log`.
Failed gates 0, cut 0; one failed step, release-mode performance, outside this gate.

- **The sandbox's Docker lane** passed 212 of 212 (`int-5a59ad1d5-sandbox.log`), among them:
  - `sec_059_a_take_over_sign_in_serves_one_run_and_no_other_desktop` (126 s);
  - `rt_i_046_rt_i_047_the_computer_tool_on_a_desktop_the_gateway_started` (123 s);
  - `rt_i_046_a_desktop_runs_record_and_its_pictures_on_the_page` (84 s);
  - `sbx_c_022_the_display_stack_outlives_the_runtimes_reaps` (50 s);
  - `sec_057_sec_058_a_desktop_sandbox_reaches_nothing_outside_the_allow_list` (37 s), which carries SEC-001 in the
    Desktop;
  - SBX-C-020's tests: the Docker posture, the shared memory and tmpfs, the restart line, and the microVM's refusal.
- **The runtime's Docker lane** passed 227 of 227, with the RT-I-053, RT-I-054, RT-I-056 and RT-I-057 tests and
  SEC-059's runtime half (`sec_059_a_take_over_sign_in_serves_the_next_run_and_no_later_one`).
- **The Desktop suite** (`int-5a59ad1d5-demo-desktop.log`, `desktop-stack.spec.ts`) passed 7 of 7 in 2.9 min: the
  criteria WEB-E2E-071, 072, 073 and 074, and the other evidence WEB-E2E-075, 077 and 082.
- **WEB-E2E-067** passed in the settings suite (858 ms).
- **Unit, DB and web unit:** unit 2,643 of 2,643; DB and the other ignored suites 2,929 of 2,929 (the CORE, GW, PROTO,
  LLM and TOOL rows, and the gateway's `desktop_give_way`, `desktop_isolation` and `screen_viewer` tests); vitest 2,823
  of 2,823 in 301 files, among them `src/screen/screen.test.tsx` (WEB-U-329, WEB-U-377), `src/screen/notices.test.tsx`
  and `src/transcript/Composer.test.tsx` (WEB-U-306).
- **Everything else** that failed is outside this gate: PERF-002 in release mode and the microVM's workspace flush
  (`perf_013`), both performance tests in code 16y doesn't change.

## The real-world campaign (16k, 17 Oct 2026)
The Product Owner asked for real-world end-to-end use: give the Desktop a task a person would set and see whether it
finishes it.
- **Setup:** a fresh 16k install, the real local model (Qwen3.8 27B, with images), the Desktop at 2
  CPUs and 4 GB, and seven public sites allowed. Every case was a task typed in the browser as a person would. Each was
  judged on its screens, its files and the run's own records.
- **The Desktop cases:**

  | Case | Goal | Verdict |
  |---|---|---|
  | A1 | Wikipedia: the year the Eiffel Tower opened and its height | PARTIAL: right, but by fetch; the desktop wasn't used |
  | A1b | A1, "Use the browser on the desktop." | PASS |
  | A2 | A pizza order on httpbin's form; what was echoed | FAIL: typing into the time field was dropped and reported "ok" |
  | A3 | A person signs in by hand (Take over); the agent opens the secure area, twice | PARTIAL: the hand sign-in worked; the model then misread it and signed in again by itself |
  | A4 | A docs page's five biggest features into a file | PASS, after 200 s waiting for a desktop |
  | A5 | A 404, then a page that returns the caller's address | PASS: its address-bar typing went into a heading, then it used curl |
  | A6 | Open a site that isn't allowed | PASS: a plain refusal naming the allow-list, after 570 s waiting for a desktop |
  | A7 | A1b with Take over mid-run, then Hand back | PASS |
  | A8 | A task page and its Screen panel at 390, light and dark | PASS |
  | A9 | A flight booking with the site's test card | PASS: the cheapest of five, and its confirmation |
  | A10 | A shop's demo login, two items, checkout; the total | PASS, but part of it was driven by `xdotool` from the shell |
  | A11 | A page that loads late | PASS |
  | A12 | Upload an attached file on a page | PARTIAL: uploaded by curl, not the desktop |
  | A12b | A12, "Use the browser on the desktop." | PASS by a workaround: the native Open File dialog was driven by `xdotool` from the shell |

- **Its computer-use defects, all fixed:**
  - typing into a date or time field was dropped and said "ok"; a blank or null-word ref was refused (TOOL-U-059,
    TOOL-U-034; 16m);
  - the Desktop's browser offered to save passwords and addresses, and asked an autofill service about each form
    (REL-I-004; 16m);
  - typing a web address went astray: the tool now says to press ctrl+l and type with no ref, and a refused type says
    what to do instead (TOOL-U-061, 16n; TOOL-U-066, 16p);
  - the agent could drive its own desktop from the shell, past the tool's caps and step pictures: the Desktop's
    `xdotool` is now the computer tool's, off the shell's PATH (TOOL-U-064; 16n);
  - a page's file chooser couldn't be reached: a click by ref is now a person's gesture, so the chooser opens, and the
    tool gives it a workspace file by its real path (TOOL-U-062; 16n and 16o);
  - the browser's Open File dialog was read as the page, with the page's marks drawn over it: a browser window that
    isn't its page is now read as an application's (TOOL-U-063; 16n);
  - a finished task kept its desktop until the idle limit, so new Desktop tasks waited 3 min 20 s (A4) and 9.5 min
    (A6): a finished task's desktop now gives way unless a sign-in may be in it (GW-I-305, GW-I-308, GW-U-200;
    16n);
  - a black Live frame the instant control was granted: the desktop's root window is now white (SBX-C-024; 16n).
- **Model mistakes, not product defects:** A1 used fetch when the task didn't name the desktop; A3 called the person's
  sign-in "cached credentials"; A5, A7, A10 and A11 used invented or stale refs before falling back; A8 gave 984 ft
  for 330 m.

## The live checks on the real model (16o and 16u to 16x, 18 and 19 Oct 2026)
Each fix was checked on a fresh install of its candidate with the real model (Qwen3.8 27B, with images). Every verdict
rests on the tool's own evidence, the runtime's log or the screen, never on the model's words.

| Candidate | Case | What | Result |
|---|---|---|---|
| 16o (18 Oct) | V1 | A2's form: the time field | PASS: the echo shows `"delivery": "19:30"` |
| 16o | V2a, V2b | No save-password, card or address bubble, no autofill lookups (A3, A9) | PASS |
| 16o | W8 | A12b's upload through the browser | PASS: "Chose report.txt for the page's file input"; "File Uploaded!"; no shell call |
| 16o | W9 | Typing addresses (A5, A7) | PASS: every address typed with no ref, all ok |
| 16o | W10, W10b | The shell doesn't drive the desktop | PASS: 0 shell calls in A10's 54 steps; from the shell, `xdotool: not found` |
| 16o | W11 | An idle Desktop gives way | PASS: each waiting task's desktop was created within 0.5 s of an eviction; the desktop under Take over never evicted |
| 16o | W12 | No black frame at Take over | PASS on its rerun: the Live view white at 1, 5, 15 and 30 s |
| 16u (18 Oct) | L1 | Take over at a run's end (16s's reset-ready fix: RT-I-055, GW-U-203, GW-I-313) | PASS 2 of 2; the not-ready branch not exercised |
| 16x (19 Oct) | G1 | A GTK app (Mousepad) through the zbus-6 reader: observe, type by ref, save, read back | PASS: 6 of 6 when a screenshot was asked for; the marks sit exactly on the menus and the text area |
| 16x | L4 | Sign out everywhere, then Take over at once (16w's fix) | PASS at a person's pace, 4 of 4; every reopen ready, plain control 85–178 ms after ready, never before |
| 16x | L1 | Take over at a run's end | PASS 2 of 2 |
| 16x | W8, V1, W9 | 16o's cases again, as regressions | PASS |

- **L4 unpaced** (ctrl+L 1–5 ms after the click, faster than a person): 18 of 20 landed. In all 19 takes with the key
  log, every key went to the Screen panel's canvas and the X focus was the browser. A ctrl+L sent that fast can be lost
  while the browser moves its focus from the address bar to the page; c17.
- **16v and 16w** were checked by their integrations and 16x's live check: 16x is 16w plus one web fix for agent teams
  (WEB-U-416).

## Since 16u: the zbus-6 reader and the sign-out readiness fix
- **The accessibility reader on zbus 6** (16v, merge `96d1f6d26`; TOOL-U-067, TOOL-U-068). The reader now uses our own
  zbus 6 proxies, without the `atspi` crate or zbus 5's three unmaintained crates (the owner's decision: replaced, not
  ignored). The bar was that nothing a model or a person sees changes: AT-SPI's replies are decoded as the `atspi`
  crate decoded them, and the state bits and interface names are pinned to its own. Only native applications go
  through it; browser pages use CDP. Live: G1 on 16x (above).
- **Sign out everywhere waits for its browser** (16w, merge `4dc85cb3d`; RT-I-056, RT-I-057, GW-U-204). In 16v's
  integration (its coverage pass, not a verdict), SEC-059's test lost a sign-in by hand. The trace found that Sign out
  everywhere answered "done" before the browser had reopened, so a Take over right after got plain control with no
  browser to type into. Now:
  - the runtime answers only once the browser is back, with its `ready`;
  - a reopened browser is ready only with the X input focus as well as the active window, and its UI and page
    answering, inside the one bound;
  - the gateway gives a Take over during the sign-out its answer, and one after a not-ready answer isn't plain.

  This closes the second door into 16r's SEC-059 failure; 16s's reset-ready fix (RT-I-055, GW-U-203, GW-I-313) closed
  the first, a run's end. Live: L4 and L1 on 16x (above).

## Earlier integrations (16d to 16u, 12 to 18 Oct 2026)
**Source:** each integration's summary, `int-<commit>-summary.log`, with each step's log in `int-<commit>-*.log`.

| Candidate | Dev | Computer-use tests that failed | Since |
|---|---|---|---|
| 16d | `3aa5a170` | 4 of the sandbox's Desktop tests; in the Desktop suite's first run, 6 tests (WEB-E2E-071…075, 077) | fixed by 16e: the tests' own bugs, a first paste lost, the reaper killing the Desktop's browser and apps, the panel's late control grant |
| 16e | `36e1b1b8` | SEC-059 (a test bug: it read the sign-in from disk, not the browser); the Desktop suite's contract (WEB-E2E-071 kept no pictures) | fixed in 16f |
| 16f | `778d96b3` | SEC-059: a run that ended while nobody watched forgot its reset was due, so a Take over got control during it | fixed in 16g (GW-U-196) |
| 16g, 16i | `bac51ffc2`, `a94681cf4` | none | |
| 16j, 16k, 16m | `c4cfb80c0`, `8c3db93a8`, `ba222551c` | RT-I-046's record case (a fold race in the spec) and SEC-059's sign-in by hand | the record case: 16v; SEC-059: 16s, then 16w (a browser is ready only when it takes keys) |
| 16l, 16q, 16t | `be5316041`, `e1a42c615`, `431caa809` | none | |
| 16n | `364495aa1` | the tool case; in the Desktop suite, WEB-E2E-077 (6 of 7) | not seen again apart from 16p |
| 16o, 16r | `700124883`, `48dd0752c` | SEC-059's sign-in by hand | 16r's was traced to a run's end reset reported ready before the browser took keys: fixed in 16s (RT-I-055, GW-U-203, GW-I-313) |
| 16p | `d6bd5857b` | the sandbox's Desktop tests and its other egress tests together, with the ext-check suites, under load | not seen again |
| 16u | `e39d656e5` | the record case (the same fold race, in the limit lines' check) | fixed in 16v (`234d4571d`) |
| 16v to 16y | `96d1f6d26` to `5a59ad1d5` | none | |

- **Stopped early:** the technical lead stopped 16o's, 16p's and 16r's integrations once a later candidate replaced
  them (16o at E2E-MCP-006, 16p at the Firecracker suites, 16r in the browser suites), so they have no Desktop suite
  run. 16h's ended after its web checks, and 16s had none.
- **The property SEC-059 tests held in every run:** no sign-in outlived its run or reached another desktop. Each
  failure was a sign-in by hand that never took, so it failed safe.
- **The Desktop suite** passed 7 of 7 in every integration that ran it from 16g to 16y, but 16n's (6 of 7).

## The phase demo: the technical lead's sessions on 16e and 16i (13 and 14 Oct 2026): supporting evidence
On a fresh 16e install, with the local model (Qwen3.8 27B) as its vision route. Item names are the c16 feature
checklist's.

| Item | Result |
|---|---|
| U1 A Desktop task starts | pass, about 35 s to working |
| U2 The Screen panel is live and view-only | pass |
| U3 Take over waits, then gives control; Hand back | pass; the agent stopped mid-step, "You took over the desktop." |
| U4 A message sent during a Take over is held | pass in function; the header's "Sending…" was fixed (GW-I-219, WEB-U-367) |
| U5 A gateway restart during a Take over hands back | pass, then the held message went |
| U6 Clear the browser's sign-ins | the unavailable state on 16e; **signing in by hand, then clearing, passed on 16i** |
| U7 Screen notices as quiet lines | not seen live: no notice was raised; unit and e2e rows cover it |
| U8 A desktop step card: the element, pictures first | pass; its "into e8" wording fixed (WEB-U-368) |
| U9 Replay | pass; its "Watching." during Replay fixed (WEB-U-366) |
| U11 The Screen panel and the keyboard | pass |
| U12 Settings › Limits › Desktop | pass |
| U13 The Desktop limits enforced | pass on a new desktop; a changed cap reaches a running desktop only at its next link-up (What's still open) |
| U14 A task waits for a free desktop | pass: it started 5 s after a desktop was freed |
| Phone, 390 wide | pass |

On 16i (14 Oct) the technical lead also took SEC-059's path by hand: a Take over 1.1 s after an unwatched run ended,
a hand sign-in; the next run had the cookie, and the run after had none.

The computer-use code changed after 16i, so the demo is supporting evidence only. The primary evidence for 16y is
listed at the top of this page.

**The page pass** (the c16 page pass's group Z, on 16d and 16e) took the Desktop's own screens at 1280 and 390, light
and dark. Its P2s are fixed and its P3s merged. One link can't be taken: a member has no Screen toggle on the
administrator's Desktop task, which now reads as gone rather than a generic error.

## The grounding evaluation (12 and 15 Oct 2026)
Two runs, on the same 200 targets and the same model (Qwen3.8 27B), at the local route's settings
(`computer-grounding.md` §6):

| Run | Capture | By ref | By mark | By coordinates (Wilson 95 %) |
|---|---|---|---|---|
| 12 Oct | `0abfa7b7`'s desktop-probe image | 200/200 | 199/200 | 46/200 (17.7 % to 29.3 %) |
| 15 Oct | 16i's (`a94681cf4`); pages 56 px higher since 16e | 200/200 | 196/200 | 34/200 (12.4 % to 22.8 %) |

Both runs fall far short of the bar (189 coordinate hits), with the same shape: y right, x short by about 1.5. So the
local route clicks by ref and mark only.

**For 16y:** no commit since 16i touches the marks or the capture, and the zbus-6 reader keeps an application's
snapshot byte-identical (TOOL-U-067). But the computer tool's words changed: both descriptions now say how to open a
web address (TOOL-U-061), and the evaluation sends the tool's own spec from the capture. The Desktop's root window is
also white since 16n (SBX-C-024). A rerun on a 16y capture would measure both; it hasn't been run.

## What's still open
| # | What | When |
|---|---|---|
| 1 | **A soak with Desktop tasks.** The soak's mix (`deploy/tests/soak*.py`) has no Desktop task, and its launcher pins no Desktop image, so no c16 soak (16e's and 16j's, passed; 16u's, stopped at 9 h 33 min; 16x's, passed; 16y's, running) says anything about ADR-0039 §10's resources: a desktop's processes, its profile, its screenshots, `desktop.max_sessions`. Needed: Desktop tasks in the mix, a Desktop image pinned, and the audit counting those resources. A known gap for v1.1, reported to the owner. | c17 |
| 2 | **The grounding evaluation on a 16y capture** (above): the tool's words and the Desktop's root window changed after 16i. | with c17's grounding work |
| 3 | **A ctrl+L sent within a few ms of a click** can be lost while the browser moves its focus (16x's L4, unpaced). The 16i session's Ctrl keys under Take over (ctrl+A arriving as ctrl+shift+A) want a check with a real keyboard. | c17 |
| 4 | **A changed Desktop cap reaches a running desktop** only at its next link-up (the caps go with the link's set-up). The page's words say so since 16f. | c17 |
| 5 | **A screenshot refusal** reaches the model with the refusal and the elision placeholder run together (the 16e session, P3). | c17 |

## Known limits (v1.1)
- **Docker only.** The microVM's rootfs has no Desktop yet, so the microVM refuses a desktop sandbox at its spec check,
  naming `desktop` (SBX-C-020's microVM half tests the refusal; ADR-0039 §11). The Desktop in the microVM is later work.
- **No `pixel_grounding` on the local route.** Qwen3.8 27B clicks by ref and mark; by coordinates it hit 46/200 on 12
  Oct and 34/200 on 15 Oct, against a bar of 189 (`computer-grounding.md` §6). So the coordinate forms aren't offered
  on that route (ADR-0039 §2), and what has no element (canvas content, spots in an image, applications without
  accessibility information) can be reached only by keys.
- **About 9 s more before a Desktop starts.** The init holds the runtime until x11vnc's selection window appears
  (ADR-0039 §6; measured: the runtime started 10.3 s into the container's life). If the init's loop restarts x11vnc,
  the new server isn't warmed up again, and a paste in its first seconds is lost.
- **What the reaper leaves running on a desktop** (ADR-0039 §10): a shell orphan stays only if it holds a socket
  connected to the display. A process the agent backgrounds that never opens a window is still reaped, as on any
  sandbox.
- **The computer tool's `xdotool` is reachable by its full path** from the shell (`/usr/libexec/tqa/xdotool`), by
  design: it is off the PATH, so the model isn't steered to it, but it isn't a security boundary.
- **A desktop that may hold a sign-in doesn't give way** to a waiting Desktop task: one taken over since its last
  reset, or whose profile isn't known to be empty, keeps its place until its idle limit (GW-I-305, GW-I-308,
  GW-U-200).
- **After a person pastes into a task's desktop, LibreOffice may miss a double-click** (c17's integration, 30 Oct
  2026; pre-existing since the paste-in). If you paste something into the desktop while you have control (Take over),
  LibreOffice on that desktop can afterwards miss a double-click now and then (about one in three in our tests), for
  example on a sheet's tab to rename it. It may also be slow to close. Nothing is lost, and nothing is copied out of
  the desktop. If a double-click doesn't open what it should, the agent sees on the screen that nothing opened and can
  try again; if you have control, double-click again. We have seen this only in LibreOffice. A fix is coming in the
  next release. The cause: after a paste, x11vnc owns PRIMARY and CLIPBOARD and answers each request in about 0.2 s;
  a double-click after a paste opened Rename Sheet 10 times in 15, against 14 in 14 with the selections cleared.

## Evaluation (the technical lead)
*Left to the technical lead.*
