# ADR-0039: Computer use: a desktop the agent sees and operates

## Status

Accepted (5 Oct 2026, with the owner's answers to its five questions; design, DEV-PLAN Phase 11). Written before any
code, with its own security review, as ADR-0034's slice contracts were: the sandbox and gateway side with the security
review, and the runtime and tools side with the slices.

## Date

2026-10-05

## Context

Some general-purpose tasks need a screen, not only a shell: a site with no API, a web form, a spreadsheet in an office
application, checking how a document renders. Today the Workstation template has a headless Chromium driven through
Playwright (ARCHITECTURE §4.1), which covers much of the web but no desktop application and gives the person no live
view. ARCHITECTURE §4.7 sketched a desktop for a later phase; this record decides it, with its own security review,
before any code (DEV-PLAN Phase 11).

The owner's principles hold:
- **General purpose.** A desktop is one more environment a task can switch to, not a separate product.
- **No approvals.** The network allow-list is the only guardrail (ADR-0035). There is no "confirm this click"; the
  person can watch any desktop live and take it over.
- **Keep it simple.** One display stack, one relay, the existing template and switching machinery.

A desktop session is a sandbox like any other. It reaches only what the egress allow-list allows, no credential of
the gateway's enters it, it writes only its workspace (and its own state), and no person sees another's sessions. What
is new is a screen that people watch and can type into, screenshots stored with the run, and sign-ins made by hand.
Those are what the security review below is about.

## Decision summary

- **An environment, not a product.** A first-party **Desktop** template (Xvfb at 1280×800, a window manager, a headed
  browser and a few applications) declares `desktop`; a task switches to it like any environment, by the person's
  choice in the environment picker. (Amended 10 Oct 2026 from the owner's decision: the agent no longer suggests the
  switch; see section 8.)
- **One `computer` tool, structure first:** DOM refs in the browser, accessibility (AT-SPI) refs elsewhere, pixels only
  as the fallback and only on routes with vision. A route with its own computer-use tool gets that tool for pixels, on
  the same executor. The model gets the last 3 screenshots as images, older ones as a line; two per-run caps bound the
  cost.
- **The person watches live and can take over.** The Screen panel relays the desktop on the single public origin to
  its owner only, view-only enforced at the gateway. Take over cancels the run as Stop does and hands input to the
  person; sign-ins happen there and last for the run that follows.
- **No new guardrail.** The network allow-list bounds a desktop as any sandbox (ADR-0035); no clipboard crosses the
  relay; nothing in the sandbox listens on a network address.
- **A record that outlives the sandbox.** Every action keeps before and after screenshots in a store with a retention
  setting, named by `screen.capture` events: protocol revision 14. The bridge and the tools need no revision.

## Decision

### 1. The desktop on the Workstation template

- **A capability, in its own template.** `desktop` is a declared capability (ARCHITECTURE §4.7: declared, never
  detected). It ships as a first-party template **Desktop**, derived from Workstation's image, rather than inside
  Workstation itself: the display stack adds several hundred MB and needs more memory, and most tasks don't want it.
  Desktop declares `shell`, `python`, `node`, `build`, `browser` and `desktop`. An administrator may derive custom
  desktop templates from it as from any template (§15).
- **The display stack**, pinned in the image and listed in its SBOM (ADR-0017, REL-I-001):
  - Xvfb at a fixed 1280×800, 24-bit, one screen, started by the sandbox's init, not by the agent;
  - a light window manager, D-Bus with the AT-SPI accessibility flag set at session start, `xdotool`, common fonts;
  - a headed Chromium (the Workstation's, with renderer accessibility on) and a few applications (a terminal, a file
    manager, an office suite);
  - a VNC server bound to the sandbox's **loopback only**, with no password. The only way in from outside is the
    bridge (section 5), which the runtime dials with its own token to a relay that serves the owner only. A password
    would have to reach the page, since RFB authentication runs end to end, and would protect nothing the loopback
    doesn't. Nothing in the sandbox listens on a network address.
- **Who starts what.** The sandbox's init starts the display, the window manager and the VNC server before the
  runtime, as the same unprivileged user. The agent can't restart or reconfigure them except as that user could
  any process, and a crashed display is restarted by the init (and reported as a tool failure, not a run failure).
- **Licences (ADR-0017).** Everything the Desktop image adds is a Debian package from the same pinned distribution,
  installed with `apt`, exactly as the Workstation image's Chromium (the `chromium` package), `bash` and `git` are:
  Xvfb, the window manager, D-Bus, at-spi2-core (LGPL-2.1), xdotool, the VNC server, Mousepad and the office suite.
  Each runs as its own program and none is linked with project code, so ADR-0017 point 7 applies to them as it does
  to Workstation's packages: the image is an aggregate, they are outside the allow list, and each release publishes
  the image's package list with exact versions and where their sources are (REL-I-001's SBOM). The runtime reaches
  the accessibility bus over D-Bus, a socket; the Rust client it links for that is project code and passes `cargo deny`
  like any crate. It is our own reader on `zbus` 6 (amended 18 Oct 2026, the owner's decision): the six AT-SPI
  interfaces the tool calls (Accessible, Action, Component, EditableText, Text and Value) as zbus proxies, the
  accessibility bus's address from `org.a11y.Bus`, and AT-SPI2's state bits and interface names decoded in
  `a11y/atspi_wire.rs`. It replaced the `atspi` crate (0.30), whose zbus 5 tree carries three unmaintained notices
  (`zbus_names`, `zvariant` and `zvariant_derive`, merged into zbus 6), when no atspi release on zbus 6 existed. A
  snapshot reads byte for byte as it did (TOOL-U-067, TOOL-U-068; RT-I-046 and RT-I-047 on a real desktop), except
  that a state bit or an interface newer than the reader's tables is now left out instead of failing the whole reply,
  and a child with an empty bus name and a real path is gone instead of a panic. A reference to the null path is no
  object whatever its bus name, as before. Nothing is vendored, modified or downloaded outside the
  distribution's packages; a component that would be (a binary from a project's own site, a patched build) needs
  ADR-0017's allow list instead. Point 8 concerns third-party images our compose files name, not this one.
- **The image's own checks:** REL-I-003's kind of test for the Desktop image (no package manager left in the final
  stage beyond Workstation's, the versions pinned), and SBX-C rows that the display, the window manager and the VNC
  server run as the sandbox user and listen on loopback only.

### 2. The screenshot and input tools

One built-in tool, `computer`, registered only when the session's environment declares the `desktop` capability
(capabilities are declared, never detected, ARCHITECTURE §4.7). It takes one `action` and that action's fields, like
the providers' own computer-use tools, so a model that knows those knows this. Its schema is strict (TOOL-U-037's
rule). It is not parallel-safe: the screen has one pointer and one keyboard.

**Structure first** (the order ARCHITECTURE §4.7 sets: browser DOM refs, then accessibility refs, then marks on a
screenshot, then pixels):
- `observe` answers a snapshot of the focused window as text, in an untrusted-content block (`source="screen"`, section
  3). Inside the headed browser it is the page's DOM and accessibility snapshot, read through the browser's local
  debugging connection, which listens only inside the sandbox. In any other application it is the AT-SPI tree (role,
  name, states, value). Each actionable element gets a **ref** (`e1`, `e2`, ...), and the snapshot names its `source`
  (`dom` or `a11y`) and its `snapshot` number.
- `click`, `double_click`, `type` and `scroll` take a `ref`. The tool performs the element's own action (press,
  activate, set text, focus, scroll into view) and falls back to XTEST input at the centre of the element's extents. The
  model never sees coordinates on this path.
- **A ref lives for one snapshot.** Each `observe` starts a new snapshot number, and an action names refs of the latest
  one only. A ref from an older snapshot, or one whose element has gone, is refused (`stale_ref`), and the result tells
  the model to observe again. Refs never carry over a window change: focusing another window ends the snapshot.

**Marks, between structure and pixels** (set-of-marks; amended 5 Oct 2026 from the owner's question: how does a vision
model know where to click? The tool side's design, folded in). Models that see an image well still place a click
poorly from raw pixels, so on a route with `vision` the screenshot carries numbered marks and the model answers with a
number, not a point:
- **A screenshot is also an observation.** `screenshot` first takes a fresh snapshot, as `observe` does (a new snapshot
  number, so older refs go stale), then answers the screen as an image with a **mark** over each of the snapshot's
  elements that has a box on the screen: a 2 px outline of the box and a label holding its ref's number (`e7` is
  labelled "7"). The result's text is that snapshot as `observe` answers it, so the numbers in the picture and in the
  text are the same elements, in the same `source="screen"` untrusted block.
- **The model clicks a mark by its ref:** "click 7" is `{"action": "click", "ref": "e7"}`, and the tool also reads a
  bare number as a ref (`"7"` is `e7`), since that is what a model reads off the picture. A marked click goes the
  structure-first path (the element's own action, with XTEST at its centre as the fallback), never to where the label
  was drawn, and lives for one snapshot (`stale_ref`). The model needs no coordinates for anything that has a mark.
  (Amended in slice 3, from its review, decided by the technical lead.)
  - A bare number is accepted only while its screenshot's snapshot is the latest. Otherwise it is refused
    `stale_ref`: "mark 7 belongs to an earlier screenshot; take a screenshot or observe again".
  - A result names a mark as "mark 7 (e7)".
- **Where the boxes come from:** in the browser, one DOMSnapshot call gives every node's layout box, mapped to the
  screen by the window's position; in another application, each listed node's AT-SPI extents in screen coordinates.
  An element without a box (off screen, hidden, zero size) gets no mark but keeps its ref in the text.
- **Drawing:** on the display's framebuffer file (`/run/tqa-display/Xvfb_screen0`, section 10). Labels are filled boxes
  with digits from a built-in bitmap font, in alternating colours, kept inside the screen and moved off a neighbour's
  label when they would overlap; at most 300 marks, as many as a snapshot lists. The PNG comes from a small encoder
  over `miniz_oxide`, already in the tree, so marks add no crate. A route with a smaller largest image gets the
  picture scaled after drawing, with labels drawn large enough to stay readable at that size.
- **The record stays unmarked:** the before and after captures (section 9) record the screen as it was; only the
  model's own screenshot is marked, and its `screen.capture` says so (`marked: true`).
- **Budgets:** a marked screenshot counts as one screenshot toward `computer.max_screenshots_per_run` and as
  `IMAGE_TOKENS` in the window, like any other (section 4).

**Pixels, the last fallback** (canvas, charts, custom widgets, apps without accessibility):
- `screenshot` answers the screen as an image (section 3), with its size, marked where the snapshot has boxes.
- `click`, `double_click`, `drag` and `scroll` take `x` and `y` in that screenshot's pixels instead of a `ref`;
  `drag` takes a `to` point as well. The tool maps them back to the display by the screenshot's scale factor, so the
  model always works in the image it saw. A point outside the image is refused (`invalid_args`, naming the field).
- **Raw coordinates only for a model that grounds well.** The coordinate forms are offered only on a route that
  declares `pixel_grounding = true` (a per-route setting an administrator sets, carried to the runtime in the route
  snapshot, protocol revision 14), or that has its provider's own `computer_tool`, which is trained on coordinates. On
  any other vision route the schema leaves them out, and the marks are how the model clicks what it sees. The residual
  limit: canvas-drawn content with no element in the tree has no mark, so on a route without pixel grounding the
  model reaches it only through the keyboard. A screenshot with no marks says so in its text ("no element has a
  mark: reach what you see with keys", and "or by x and y" on a route with `pixel_grounding`).
- Pixel actions and marks exist only on a route with `vision`. Without it the schema leaves out `screenshot` and the
  coordinate fields, so the model is never offered what it can't use (section 3).
- **Grounding is measured per model** (`docs/evals/computer-grounding.md`, which has the targets' table and their
  kinds): 200 targets on fixed screens (browser pages and a few desktop applications, some with canvas content), each
  asked three ways on every candidate route: by ref from the text alone, by mark from a marked screenshot, and by
  coordinates from an unmarked one (a hit when the point falls inside the target's box). The screens are captured once
  and replayed, so every route sees the same pixels. The result is a hit rate per way per route, with Wilson
  intervals. **Decided** (the technical lead, 5 Oct 2026, on the owner's delegation): a route gets `pixel_grounding`
  only when its coordinate hit rate's Wilson lower bound (95 %) is at least 90 %, which over 200 targets means at
  least 189 hits. The admin guide lists the routes measured and their numbers.

**Typing and keys:** `type` takes `text` (at most 4,000 bytes; with a `ref`, into that element; without one, into the
focus). `key` takes `keys`, an xdotool-style combination (`ctrl+s`, `Return`, `alt+Tab`), at most 8 keys. Text reaches
the application as XTEST key events or the element's set-text action, never through the clipboard (section 7 decides
the clipboard). **Amended 12 Oct 2026 by the technical lead:** on the desktop, `key` and `type` without a
`ref` always go through XTEST, as a person's keyboard does, whichever window has the focus, so the browser's own UI
(ctrl+l and its address bar) gets them as well as the page. Slice 2's driver sent them to the focused page as debugging
protocol input events, which Chromium's UI never sees, so a model's ctrl+l and URL left the page on `about:blank`. The
browser's debugging connection stays for reading the page, its refs and their actions (TOOL-U-057).

**Timing:** after every action the tool waits until the screen settles: no change for 300 ms, or 3 s at most.
`wait` takes `ms` (100 to 10,000) for things that take longer, such as a page load. An action's result says what
changed: the window title, a new window or dialog, or "no visible change". It never claims a success it can't see.

**Every action keeps its evidence:** a screenshot before and after each acting action is captured for the record
(section 9 stores them; the contract below names the event). These are the person's replay and audit trail, and are
apart from what the model is sent (section 3).

**A provider's own computer-use tool** (ADR-0019's transparent relay). A route may declare `computer_tool` naming its
protocol's own tool, which today means only Anthropic Messages' computer-use tool at the version the route names. On
such a route the runtime offers that tool for the pixel actions, with the display's size, and the `computer` tool keeps
only its structure-first actions (`observe` and the ref forms). There is one executor underneath: the provider's actions
(screenshot, click, drag, type, key, scroll, wait, zoom) map onto the same XTEST calls, the same settle rule and the same
evidence. A route without `computer_tool` gets the whole `computer` tool. Either way the model has one way to do each
thing.

### 3. How the model sees the screen

- **Images on vision routes.** A `screenshot` result, and the provider tool's, carries the image beside its text, at
  the display's size (1280×800, section 1) or scaled down when the route declares a smaller largest image, with the
  long edge never above it. The scale factor is kept with the result, so coordinates map back (section 2). The image
  bytes are never journaled or logged, as with attached images today (FR-36): the journal holds the screenshot's stored
  file reference, and the bytes are read when the request is built.
- **How each protocol gets it.** The neutral request carries the image on the tool's result message (`tqa-llm` already
  allows content parts there). Anthropic Messages puts it inside the `tool_result`. OpenAI Chat and Responses, which
  take no image in a tool result, get the tool result's text and then one user message holding only that image,
  labelled as the result's screenshot, never as the person's words.
- **How many are kept.** The request carries the images of the last 3 screenshots of the run. Older ones go as one
  fixed line ("[screenshot after step 12, not resent]"), so the history stays readable and its cost stays flat.
  Only the latest screenshot carries its marks, and only while no later `observe` has replaced its snapshot; the older
  ones go unmarked. The runtime keeps each marked screenshot's clean copy beside it for this.
  Compaction drops the images with the turns it summarises. A screenshot is counted as `IMAGE_TOKENS` (1,600) in the
  window, as attached images are.
- **Without vision.** The structure-first actions work on any route, local models included: a snapshot is text. The
  pixel actions aren't offered (section 2), so the model reaches canvas-drawn content only through what its accessibility
  tree says. The system prompt says the environment has a desktop and which actions this route has.
- **Screen text is untrusted input** (SEC-034's rule for web pages): every snapshot, every element name and value, and
  every action result's window title goes inside the untrusted-content marker with `source="screen"`. The system
  prompt says text on the screen is data, never instructions. A screenshot can't be marked inside the image, so its
  label says the same in words. This is a note in the model's input, not a gate (ADR-0035).

### 4. Budgets

- **Every action is a tool call,** so it counts toward the run's steps and its token budget as any tool call does.
  Desktop tasks take many small steps, so a task whose environment declares `desktop` gets a default of 500 steps
  (the owner, question 4). The person's own controls are its budget and time; the step cap stays as a bound because
  some model servers report no usage, which leaves the token budget nothing to count.
- **Images:** at most 3 in a request (section 3), each counted as 1,600 tokens, so a request holds at most 4,800
  image tokens however long the run is. Resending all of a run's 100 screenshots would put 160,000 into its last
  request alone.
- **A per-run cap on screenshots sent to the model:** `computer.max_screenshots_per_run`, an administrator's runtime
  setting (ADR-0028), 1 to 1,000, default 200. Past it, `screenshot` is refused (`screenshot_limit`) and the result
  tells the model to work from `observe`. It bounds the token cost a looping model can run up.
- **A per-run cap on actions:** `computer.max_actions_per_run`, 1 to 5,000, default 1,000. Past it every action is
  refused (`action_limit`, its text ending "Stop using the desktop and tell the person what is left."), and the run
  ends as on any repeated failure. Evidence screenshots (section 2) count toward
  this cap, not the screenshot cap, since the model isn't sent them.
- **Both caps survive a resume.** A resumed run's tool starts from what the run's journal shows it spent: each
  screenshot that came back, and each action past its checks. It's read back from the journal, so there's no second
  store to drift.
- **What the person sees:** the run's usage counts screenshots sent and actions taken, beside its tokens.

### 5. The live Screen panel

- **The relay.** The page opens `GET /api/v1/conversations/{id}/screen` as a WebSocket on the single public origin
  (ADR-0022): the session cookie, the exact `Origin` check, no published port. The gateway pairs it with one of the
  sandbox's screen bridges (below) and relays RFB bytes both ways.
- **One bridge per viewer** (amended in slice 5, with the runtime side). An RFB session belongs to one connection, and
  x11vnc (`-shared -forever`) serves each viewer on a connection of its own. So each open viewer is paired with its own
  bridge: at most four per sandbox, as there are at most four viewers. View-only applies per connection: the
  gateway's filter drops input on every bridge but the Take over holder's (below), and the runtime's side stays a
  plain byte copy.
- **The bridge dials out, as the link does.** Nothing in a sandbox listens on a network address. The runtime of a
  `desktop` session opens a second outbound WebSocket to the gateway, `/runtime/screen`, with its own runtime token
  (the link's authentication, bound to the conversation and its sandbox), and bridges it to the VNC server on the
  sandbox's loopback. It works the same on both backends, through the address the runtime already uses for the link.
  The runtime side of the bridge is in section 5's runtime half.
- **An unwatched desktop sends nothing.** The gateway holds the bridge's dial as a long poll: it upgrades the request
  (`101`) only when a viewer is open or opens within 30 s, and otherwise answers `204` at 30 s, and the runtime dials
  again. A viewer's close closes its own bridge; when the last viewer closes, every bridge is closed and the runtime
  is back to one held dial.
  So no frame leaves a sandbox that nobody is watching, and an idle desktop costs one held request.
- **Who may connect:** the conversation's owner only. Tasks are private, as transcripts are (ARCHITECTURE §12, "One
  user reads another's conversation"; SEC-005): a person a workspace is shared with, a channel member and an
  administrator never see the screen; an administrator's metadata view says only that a desktop session exists. At most four viewers per conversation (the owner's tabs), refused past
  that with `screen_viewers`.
- **View-only is enforced at the gateway,** not by the page. Until the viewer holds Take over (section 6), the gateway
  forwards only the server's frames and the client's protocol handshake and framebuffer requests. It drops every input
  message (`KeyEvent`, `PointerEvent`, `ClientCutText`), whatever the page sends. noVNC's own view-only switch is a
  display courtesy only.
- **The relay fails closed.** It frames every viewer message; a message type it doesn't know, or one whose length
  doesn't fit, closes that viewer's socket. Nothing it can't parse passes through (GW-I-218).
- **The connection's lifetime:** a viewer's socket ends when the sandbox retires, when the conversation is deleted or
  its owner signs out, and after the session's idle limit; the page reconnects as it does for events. The relay holds
  no frame: nothing is stored from it.

**The runtime's side of the bridge.**
- **It dials** `GET /runtime/screen` with its runtime token, as a second outbound WebSocket beside the link, once the
  session's environment declares `desktop` and the display is up. It copies bytes both ways between that socket and the
  VNC server on loopback, without reading or keeping them: the runtime holds no frame buffer, and the bridge runs on
  its own task, so a slow model call never stalls the picture, and the picture never delays a run.
- **It idles without a viewer.** The gateway holds the dial: it upgrades it (`101`) when a viewer is open or opens
  within 30 s, and otherwise answers `204` at 30 s, and the runtime dials again at once. When the last viewer closes,
  the gateway closes the bridge's socket, and the runtime is back to a held dial. So an unwatched desktop sends no
  frames.
- **It reconnects** after a drop with backoff (1 s doubling to 30 s, with jitter), as the link does, and stops when the
  session ends. A bridge that can't reach the VNC server closes the socket with a reason the panel shows ("The screen
  isn't available"), and tries again on the next dial.
- **Bounded:** one bridge per viewer, at most four. The runtime keeps one spare dial held and dials again as soon as
  the gateway upgrades one. While four are upgraded, the spare's dial is refused `409`, and the runtime waits for one
  of its bridges to close before dialling again. Frames pass in chunks of at most 64 KiB; a socket idle for 60 s with a viewer
  attached is pinged, and closed if it doesn't answer.
- **What the panel shows beside the picture** comes from the run's events: each `computer` call as one line ("Clicked
  Save", "Typed 42 characters", "Pressed ctrl+s"), its result, and its before and after screenshots (the contract's
  `screen.capture`). So the page can replay a run after the sandbox is gone, from the same events the model's history
  is projected from (ARCHITECTURE §5.0).
- **Take over** cancels the active run as Stop does (section 6), so no model call is left moving the pointer under the
  person's hand. The model learns what happened from the history (the run cancelled, then the person's next message,
  such as "I've signed in; carry on"), and the next `observe` shows it the screen as the person left it. That needs no
  frame of its own.

### 6. Take over and sign-ins

- **Take over is a pause, not an approval.** The person presses **Take over** on the Screen panel: the gateway cancels
  the conversation's active run as Stop does (`run.cancel`), marks the conversation `taken_over`, and from then on
  forwards that one viewer's input to the screen. The agent gets no turn while it is taken over: no queued turn starts, whether the
  person's own message, an agent team's wake or any other held turn; each waits behind the Take over, as a message
  waits behind a run. Nothing new is needed in the protocol.
- **One controller at a time.** Take over belongs to one viewer socket. Another tab's Take over is refused until the
  first hands back or its socket closes: that tab keeps its picture and is told another holds the screen.
- **Handing back.** **Hand back** (or closing that socket) ends Take over: the gateway stops forwarding input, records a
  `screen.handed_back` notice in the transcript (who took over, for how long; never what was typed), and the next
  message starts a run as usual. The agent sees the screen as the person left it, through its next observation.
- **Sign-ins.** A person who needs the agent to work in an account signs in by hand during Take over. The agent never
  types a password the person gave it in a message, and no credential of the gateway's (ADR-0037's host keys included)
  enters the desktop.
- **Every run starts signed out, except that a sign-in made during a Take over serves the run that follows it** (the
  owner, question 2; amended by the lead, 7 Oct 2026). The desktop browser's profile (cookies, saved logins, site data)
  lives in its own tmpfs, `/run/tqa-browser` (section 10), never in the workspace or the conversation's state. When any
  run of a desktop session ends (completed, failed or cancelled, a Take over's cancel included), the runtime closes the
  browser and empties its profile; the next action starts the browser fresh. No protocol frame tells the runtime about a
  Take over: a sign-in made during one, after the run it cancelled has ended, is simply still in the profile when the
  next run starts, and is gone when that run ends. A Take over that no run follows leaves the profile until the sandbox
  retires, which ends it too. So every new sandbox, every later run, a person a workspace is shared with, a fork and
  another task on the same workspace all start signed out. There is no option to keep a session across runs in this
  version. **The Product Owner confirmed it for 17.0 (8 Oct 2026)**, after a computer-use review (work in an
  account means a Take over and a sign-in before each message): keeping a sign-in for the whole task is for a later
  release (PRD §12, item 24).
  - **Known edge:** the cancelled run's own end empties the profile. A sign-in begun in the few seconds while that run
    is still winding down may be emptied with it; the person signs in again. This fails safe.
  - **Known edge:** the next run uses the same live browser, so the sign-in reaches it from the browser's memory.
    Chromium writes its cookies to the profile only in batches, about every 30 s, and a reset kills the browser
    without a graceful close (the profile is emptied straight after). So if the browser dies between a sign-in and
    the next run (a crash), a sign-in from its last ~30 s can be lost and the person signs in again. A runtime restart
    empties the profile anyway (below). This fails safe (amended 12 Oct 2026: SEC-059).
- **Take over always finds the runtime's browser** (the lead, 7 Oct 2026). After every reset (a run's end, Sign out
  everywhere, and the session's start) the runtime starts its browser again, blank, on the emptied profile, so the
  person signs in there and nowhere else. A browser started by hand is the same profile too: the Desktop image's
  `/etc/chromium.d/tqa-desktop-profile` gives every headed `/usr/bin/chromium` that names no profile of its own
  `--user-data-dir=/run/tqa-browser/profile`, and `x-www-browser` and `gnome-www-browser` are set to it. That covers
  every way the desktop offers to start one: the window manager's Web browser (its only browser entry; its Debian and
  Applications menus are empty, as `menu` and `obamenu` aren't installed), xterm, and a link an application opens. On
  that profile Chromium's own single-instance lock hands a second start to the runtime's browser, so a sign-in never
  lands in the sandbox's home, where no reset would reach it.
- **Every reset fails safe** (the lead, 7 Oct 2026). A run's end and Sign out everywhere stop any process still
  holding `/run/tqa-browser/profile` (a browser started by hand after the person closed the runtime's, say): SIGTERM,
  2 seconds' grace, then SIGKILL. Then the profile is emptied and the blank runtime browser started again. Only a
  holder that survives SIGKILL leaves the profile as it is: the reset reports `profile_in_use` and logs that process's
  pid and name. A driver a run still holds is `profile_in_use` too (Sign out everywhere is offered only between runs).
  - **Known edges:** a browser started by hand with its own `--user-data-dir`, or Chromium's binary run directly,
    keeps its own profile until the sandbox retires: a deliberate act, not a way the desktop offers. A runtime restart
    in the same sandbox resets at its session's start, so a sign-in made before it is emptied; the person signs in
    again (fails safe).
- **A paste in reaches the desktop's clipboard from the start** (amended 12 Oct 2026: RT-I-046's paste never
  arrived). x11vnc 0.9.17 makes the window it owns the X selections with only some 10 s into a viewer's connection
  (inside its per-client setup; no flag, `-speeds`, `-noxdamage` or `-noxfixes`, makes it sooner, and it does no X
  work without a client), and a paste it gets before then is logged as set but owned by no one, so the browser's
  ctrl+v gets nothing; only the root window's cut buffer has it. Once made, the window stays for every later viewer.
  So the Desktop's init warms the server up once, before the runtime starts: a viewer on loopback that completes the
  handshake and sends nothing else (no input, no paste, no update requested, so no frames), and leaves as soon as the
  server's selection window appears, told by its shape (nameless, 1x1, at +3+2; the window manager's own nameless
  windows sit at +0+0), at most 20 s. The runtime, and with it any person's viewer, comes only after it is gone, so no
  one shares the server with it and no viewer count or panel ever sees it (SBX-C-020 checks that once the runtime runs
  the server has no client and has its selection window). The cost is some 10 s more before a Desktop's runtime registers. `-seldir recv` stays:
  without it the desktop's cut buffer was sent to viewers. Known edge: an x11vnc the init's loop restarts isn't warmed
  again, so a paste in its first seconds is lost, as before this change.
- **Typing is offered only once the desktop takes it** (amended 12 Oct 2026: two of three hand sign-ins in a
  Take over never reached the site). Two things have to be true before the panel says "You have control" and gives the
  picture the keyboard: the gateway has granted control, and the panel's screen client has connected (noVNC's
  `connect`); until then it says "Connecting to the desktop…", input stays off, and Hand back is still there. A grant
  that came before the client had loaded used to leave it view-only under "You have control", so nothing typed was
  sent. And the run's end reset is announced only once the reopened browser's window has the focus, not as soon as its
  debugging port opens: keys sent before then reached no window, or the window manager (WEB-U-329, RT-I-053). The reset
  says whether it got that far (`screen.reset`'s `ready`, 18 Oct 2026, an integration run's `sec_059_a`): the launch and the
  focus share one 20 s bound, and a browser that didn't start or whose window never took the focus is said not ready.
  The person's control then says a sign-in now may not last, and keeps saying so until a reset says the browser is
  ready (GW-U-203, GW-I-313, RT-I-055). Before, after 5 s the reset was announced as over anyway, and under load a
  person typed into a desktop whose browser wasn't there. Ready proves input, not only a window (amended 19 Oct 2026,
  an integration run's coverage `sec_059_a`: a reset said ready, control was plain and on time, and the keys typed
  reached no page): within the same 20 s bound, the browser's window must hold both the window manager's active window
  and the X input focus, where the VNC server's keys go (the manager names a window active and moves the input focus
  as a step of its own), and the browser must answer two round trips on its debugging connection, its window asked
  through its own process (`Browser.getWindowForTarget`, any state but minimised) and its page asked whether it has loaded
  (`document.readyState` complete; about:blank counts). No key is sent to find out: it would leave marks in the
  person's browser. Each reopen logs one line with whether it is ready, what ended it, and what the launch, the focus
  and the answer took (RT-I-057).
- **Sign out everywhere** on the Screen panel's ⋯ ends the session at once (the browser restarted with an empty
  profile); the transcript notes that it was done. Its answer (`screen.sign_out.done`) comes once the browser has
  started again, with the same `ready` as a run's end reset (amended 19 Oct 2026, an integration run's `sec_059_a`
  review): the gateway treats the sign-out as a reset from the moment it asks, so a Take over meanwhile waits for it,
  and one after a not-ready answer is granted not ready. Before, the runtime answered first and started the browser
  after, so a Take over right after was granted plain control over a desktop with no browser yet (GW-U-204,
  RT-I-056). The gateway waits 30 s for the answer (the runtime's 2 s for the profile's holders and its 20 s bound);
  an unanswered sign-out leaves the task unclean and not ready until a reset says otherwise.
- **What the agent can do with a signed-in session** is what the person could, for that one run: this is the point of
  signing in, and the network allow-list still bounds where it can go.

### 7. What a desktop session may reach

- **The network:** the conversation's allow-list, enforced at the sandbox's network as for every sandbox (ADR-0035).
  The sandbox has no route except the egress proxy, so a desktop application that ignores proxy settings gets nothing,
  rather than a way round. A desktop gets the same allow-list as any other task, with nothing added for it: full access
  (the **Open web** list) stays off unless an administrator turns it on, as for every task (the owner, 5 Oct 2026;
  question 1).
- **The clipboard: paste in during Take over, never copy out** (the owner, question 3). The gateway forwards
  `ClientCutText` only from the viewer holding Take over, so the person can paste a long address or a password from
  their own password manager, and drops it from every view-only viewer. Nothing leaves the sandbox through the person's
  clipboard, because the sandbox never produces it. x11vnc runs with `-noclipboard -noprimary -seldir recv`: it only
  receives cut text and never sends `ServerCutText` to any viewer. SBX-C-020 checks the flags, and RT-I-046 proves
  it: a raw RFB client gets no `ServerCutText` while the desktop's selections are set, and its paste reaches the
  desktop. (Amended in slice 5. The gateway would have to parse the whole server stream to drop it there; stopping it
  in the sandbox is exact and cheap.) Inside the desktop, applications share the display's own clipboard
  as usual.
- **Files:** the workspace at `/workspace` (where applications save, with Downloads at `/workspace/Downloads`) and the
  sandbox's temporary space; the uploads and file panels work as for any task. No host path, device (no GPU, no audio,
  no USB) or other conversation's state is mounted.
- **Nothing else:** no access to the gateway's API with the person's session (the browser in the desktop is not
  signed in to TensorQuay), and the runtime token stays out of the agent's environment as today.

### 8. The switch suggestion (not in this version)

**Amended 10 Oct 2026 from the owner's decision, by the basic-tools principle (7 Oct 2026):** no agent tool suggests
an environment switch. The `suggest_environment` tool, its card and its refusal are dropped before they shipped (the
tool was never built; the card had no trigger on a real install). The switch stays the person's, through the
environment picker and `PUT /api/v1/conversations/{id}/environment` (ARCHITECTURE §4.7), as every environment change
is. A model that still calls the name gets an unknown tool's failure, shown as any failed step. A later ADR may
revisit a suggestion.

### 9. Screenshot retention

- **Where:** each `computer` action keeps a before and an after screenshot (section 2). They are not events: an event
  stays within the event size bound, so the runtime's `screen.capture` event (section 2's contract: id, size,
  SHA-256, before or after, and the call) names it, and the image goes to the conversation's **screenshot store**, a
  directory beside the state directory on the host's storage root (`screens/<conversation>/<id>.png`), written by the
  gateway as the runtime uploads it over the link. The event is new in protocol revision 14 (the runtime contract); a
  conversation whose environment declares `desktop` is refused at the link below 14, as a team's is below 13.
- **The upload** (agreed by both sides in slice 5, revision 14):
  - **Frames.** The runtime sends `screen.upload` frames, `{id, chunk, last, data}`:
    - `id` is the PNG's SHA-256 in lower-case hex, the same value as `screen.capture`'s `id` and `sha256`. Ids are
      content-addressed within a conversation, so a repeated upload is a no-op.
    - `chunk` counts from 0, and `last` marks the final chunk.
    - `data` is base64 of at most 96 KiB raw, so each frame stays within the link's frame bound.

    One upload is in flight per sandbox, and its chunks arrive in order.
  - **The answer.** The gateway answers with `screen.stored`, `{id, ok, reason?}`. A refusal's `reason` is one of a
    closed list:
    - `too_large`: over 512 KiB raw, refused as soon as the running total passes it;
    - `quota_full`: the conversation's one state quota (`state_bytes`: its kept screenshots and its run's state
      together) can't take it;
    - `bad_image`: not a PNG, or its SHA-256 doesn't match `id`;
    - `store_failed`: anything else.

    An upload left unfinished for 10 s after its last chunk is dropped.
  - **Waiting for the answer.** The runtime waits at most 2 s for a before capture's answer, because that capture is
    journaled ahead of the action's `tool.start` and must not stall it. It waits at most 5 s for an after capture's or
    the model's own screenshot's answer. If no answer comes in time, or the answer is a refusal:
    - the capture is dropped: no `screen.capture` event, and the action's `evidence` is false for that phase;
    - the action goes on;
    - a late `screen.stored` for a dropped id is ignored, and its file stays only until the purge.
  - **A full quota.** The gateway sends the owner one notice per run that screenshots aren't being kept because the
    task's storage is full, and never stops the run.
  - **What the caps count.** The model's own screenshot uploads the same way, marked. Evidence captures count toward
    `computer.max_actions_per_run`, not the screenshot cap (section 4).
- **Bounds:** at most 512 KiB per screenshot (the display-size PNG), refused past that as above. One quota: the task's
  kept screenshots and its run's state directory together within `sandbox.state_bytes`. A screenshot that would pass
  it isn't kept (`quota_full`), and the run's state growing past it later ends the run as a full state always has.
- **How long:** `desktop.screenshot_retention_days` (runtime-mutable, ADR-0028; default 30, 1-365). The purge removes
  older screenshots and leaves their events, which then say the image has expired. Deleting the task deletes them all
  (PRD §12 item 18), through the purge as today.
- **Who sees them:** the conversation's owner, through the transcript; the same privacy as the transcript itself.

### 10. Resource limits

- **The Desktop template's defaults:** 4 GiB of memory, 2000 millicores and 1024 processes, its own sizes in the
  environment's row when it is registered (the other built-ins take the install's defaults). Like every environment's
  size, one above the installation's `[sandbox.ceiling]` is lowered to it at start and recorded until an administrator
  saves the environment (the environment sizes design's `clamped_from`), so a small installation still offers a
  Desktop, at its ceiling. The numbers are the research note's estimate; the Phase 11 spike measures them and this
  section is amended with the result.
- **Shared memory and the browser's space** (only for an environment that declares `desktop`; other sandboxes are
  unchanged): the sandbox's `/dev/shm` is 1 GiB (Docker's `shm_size`; on the microVM the guest's `/dev/shm` is a tmpfs
  sized from its memory), and a second tmpfs, `/run/tqa-browser` (512 MiB, `nosuid,nodev,noexec`, mode 0700, the
  sandbox user's), holds the browser's profile and cache. Both count toward the sandbox's memory limit. The sandbox's
  16 MiB `/tmp` is too small for either. The runtime starts Chromium with `--user-data-dir=/run/tqa-browser/profile`
  and a 256 MiB disk cache, so it stays within that tmpfs. The spike's measurements may lower the shared memory to
  512 MiB. A third tmpfs, `/run/tqa-display` (8 MiB, `nosuid,nodev,noexec`, mode 0700, the sandbox user's; amended
  5 Oct 2026 with the runtime side), holds the display's framebuffer as a file: the init starts Xvfb with
  `-fbdir /run/tqa-display`, so `/run/tqa-display/Xvfb_screen0` (XWD, 32 bits per pixel, about 4.1 MB at 1280x800)
  is the screen itself. The runtime reads it for the settle rule's "did the screen change" digest in any application
  and for screenshots, with no other tool in the image; without it the settle rule falls back to window titles. All
  three mounts are Docker's `tmpfs` entries (the init can't mount one: it runs as the sandbox user on a read-only
  root), and RT-I-046's probe runs with exactly the same three.
- **How many at once:** `desktop.max_sessions` (runtime-mutable; default 2, 1 to the slot count) caps live desktop
  sandboxes per installation; another is queued for a slot with the reason `desktop_sessions`, as a busy workspace
  queues (ADR-0016's admission). Per person, the existing per-user slot cap applies. **Amended 12 Oct 2026 by the
  lead:** an idle Desktop is never retired to make room for a waiting one, as an idle sandbox is for a slot: its
  browser's profile is a tmpfs, so the person's sign-ins would go with it, unasked. The waiter starts once a Desktop
  ends (it idles out, its task is deleted, or the cap rises), and the waiting line says so ("One frees when another
  task has been idle for a while"; each sandbox keeps the idle limit it started with, and a task kept on never idles,
  so the line names no number) (GW-I-224, WEB-U-001). **Amended 17 Oct 2026 by the lead** (c16k real-world A4 and A6:
  desktop tasks waited 3 min 20 s and 9.5 min behind desktops whose tasks had finished): an idle Desktop gives way to
  a waiting one when its browser's profile is known empty, so no sign-in goes with it. That is: a clean run-end reset
  (or Sign out everywhere) came, and no Take over since. A hand sign-in serves the run after it (section 6), so a
  Take over since the last reset keeps the desktop until that run's end. It must also have no run active, nothing
  queued, not be pinned, and not be held by a person now. The rest is as for a slot's eviction: the least recently
  active first, one at a time, the workspace kept. The same guard holds when a task waits for a slot: a Desktop that
  may hold a sign-in isn't evicted for it either (GW-I-308); other environments' idle sandboxes are as before. What the gateway knows of resets lives in its memory, so after a
  restart every desktop counts as possibly signed in until its next reset: it fails safe, and such a desktop waits out
  its idle limit as before. The waiting line reads "One frees soon after another task finishes" (GW-I-305, GW-U-200,
  WEB-U-397).
- **What the runtime's reaper leaves on a desktop** (the technical lead, 12 Oct 2026: the first shell command of a desktop
  run killed the browser, and no application the agent opened survived the call). The runtime is a child-subreaper and
  kills every direct child it didn't record after each shell call and each run (ADR-0016's amendment, TOOL-U-008). On a
  desktop it also leaves alone: its own browser, started as the leader of a group of its own and recorded as an MCP
  server's group is (forgotten when the browser closes); and, in a session whose environment declares `desktop`, the
  clients of the session's display, so an application the agent opens stays, as on a person's desktop. A client is
  told strictly: a socket of its own is connected to the X server's socket (`/tmp/.X11-unix/X0`), as the kernel's
  socket diagnostics give each connection the server accepted its peer; `DISPLAY` in an environment counts for nothing.
  An application a call starts in the background may reach the display only after the call has ended, and a model
  doesn't know to wait for it: so in a desktop session an orphan younger than 10 s is left for a later decision, taken
  once that grace has passed, on those processes alone (the same pid and start, so a reused pid is never taken for
  it): one that became the display's client by then stays, and any other is killed. A shell call in flight meanwhile
  is untouched. Everything else is reaped as before, and the sandbox's process limit and lifetime still bound what
  stays (RT-I-054, SBX-C-022).
- **The display's own bounds:** one screen at a fixed size; the relay forwards at most 10 `FramebufferUpdateRequest`s a
  second from each viewer, so the frame rate holds whatever the VNC server does (GW-I-218); and the viewer count
  (section 5).

### 11. Both backends

- **Docker:** everything runs in the sandbox container: the display, the window manager, the VNC server on the
  container's loopback, the runtime and its bridge.
- **MicroVM:** the same, inside the guest. The guest's memory and process limits are the template's; the guest
  kernel needs nothing new (Xvfb is user space, with no GPU), and the rootfs is built from the Desktop image as for
  any template (images/rootfs). The bridge dials the gateway through the guest's link address, as the runtime does.
- **What differs:** nothing in the contract. The microVM's memory is fixed at boot, so a desktop guest's 4 GiB is held
  for its life, where a container's is a limit; `desktop.max_sessions` matters more there.
- **In v1.1, Docker only:** the microVM's rootfs has no Desktop yet, so a desktop sandbox asked of the microVM is
  refused at its spec check, naming `desktop`, and never started without its mounts (SBX-C-020). The Desktop in the
  microVM is later work, with the design above.

## Security review

Assets: the person's accounts in sessions they sign in to, their files in the workspace, their clipboard, other
people's screens and sessions, the gateway's secrets, the installation's capacity, and the screenshots (which show
whatever was on screen). Actors: the agent (with whatever a page or document on screen tells it), a page or
application it opens, the person, another person on the installation, and the model provider that receives
screenshots.

| # | Threat | Mitigation | Test |
|---|---|---|---|
| T1 | Text on screen instructs the agent (prompt injection from a page, a document, a dialog) | Screen text is marked untrusted in the model's input, like page text (SEC-034); no approval, but the person watches live and can take over; the agent reaches only the allow-list | SEC-057: injected on-screen text arrives marked, and the run's egress stays within the allow-list |
| T2 | A desktop application bypasses the egress guardrail (ignores the proxy, uses its own DNS, a raw socket) | The sandbox has no route but the proxy (ADR-0035), on both backends | SEC-058: SEC-001/006's probes from the Desktop template, with a browser launched without proxy settings |
| T3 | Someone else sees or drives a person's screen | The relay serves the owner only, checks the session and `Origin`; the bridge is bound to the sandbox's runtime token; no port is published or listened on | GW-I-217, and SEC-005 grows a case: another person, a share holder, an administrator and a wrong-origin request are each refused |
| T4 | A viewer injects input without Take over (a modified page, a script) | View-only is enforced at the gateway: input messages are dropped unless that socket holds Take over | GW-I-218: a view-only socket's KeyEvent and PointerEvent never reach the sandbox, an unknown or malformed message closes it; with Take over input passes |
| T5 | Two people or tabs fight for control | One controller; a second tab's Take over is refused, and that tab is told another holds the screen | GW-I-219 |
| T6 | A signed-in session leaks to another task or person (shared or forked workspace), or outlives the work it was for | The browser profile lives in the sandbox's own tmpfs, `/run/tqa-browser`, never the workspace or the state; it is emptied when any run ends, so it serves at most the run that follows a Take over's sign-in, and ends with the sandbox; Sign out everywhere ends it at once; the runtime's browser is open again, blank, after each reset, and every headed browser started by hand uses the same profile (`/etc/chromium.d`), so no sign-in lands in the sandbox's home | SEC-059: after the run that follows a sign-in, the next run starts signed out, as do a new sandbox, a share holder's and a fork's desktop; after Sign out everywhere the profile is gone (GW-I-222); the site's cookie is never outside `/run/tqa-browser` (`desktop_live.py sign-in`) |
| T7 | The agent misuses a signed-in account (sends, buys, deletes) | Out of scope by design: no approvals (ADR-0035); bounded by the allow-list, and the person decides whether to sign in at all; a session lasts one run (the owner, question 2) | None (a product decision, recorded) |
| T8 | Data leaves through the clipboard, or a viewer pastes into the desktop | Nothing is ever copied out: the sandbox never produces `ServerCutText` (x11vnc `-noclipboard -noprimary -seldir recv`); a paste passes only from the viewer holding Take over | RT-I-046: no `ServerCutText` reaches a raw RFB client while the selections are set, and a paste in arrives; SBX-C-020: the flags; GW-I-218: a view-only viewer's `ClientCutText` is dropped and the controller's passes |
| T9 | Another sandbox reaches a desktop's VNC server | It listens on loopback only, with no password; the bridge dials out | SBX-C-020: no listening socket on any non-loopback address, on Docker; the microVM refuses a desktop sandbox in v1.1 (section 11) |
| T10 | Screenshots expose what was on screen (a password field shown in clear, private mail) | The owner only sees them; they expire (retention); they go to the model provider only as the run's observations, like any page text, and a local model keeps them local | GW-I-221: expiry and deletion; the provider question is stated in the docs |
| T11 | Resource exhaustion (memory, processes, screenshot volume) | Template limits within the ceiling, `desktop.max_sessions`, screenshot size and quota | GW-I-224, GW-I-221 and SBX-C-020 |
| T12 | A gateway secret reaches the desktop (environment, a file, the screen) | As for every sandbox: none is passed in (SEC-001); the bridge's token is the runtime's, already inside | SEC-001's search repeated in a Desktop sandbox |
| T14 | Screen content imitates the marks (a page paints its own numbered labels) or places a decoy beside a label, to steer the model's click onto another control | A mark resolves to its snapshot element, never to a position: a click on 7 acts on `e7` whatever is painted near its label, and the result names the element acted on; the result's text lists every element with its role and name in the untrusted block, so a decoy's own words are data; labels are drawn by the tool after capture, on its own copy of the screen | RT-I-047: a page painting a fake "7" over another button; the click on mark 7 presses the real `e7` |
| T15 | Marks hide what the model needs to read, or a crowded screen is mis-read | Labels kept inside the screen and moved off each other, outlines 2 px, at most 300; the text lists the same elements, so nothing is known only from the picture; a route whose model grounds well may use raw coordinates (`pixel_grounding`), set from measured numbers | TOOL-U-048 (the overlay's rules); the grounding evaluation |
| T16 | The marks reveal more of the screen to the model's provider | Labels are digits drawn by the tool, never text taken from the screen, and the outlines show only boxes the screenshot already shows; the snapshot's text is what `observe` would send anyway. No exposure beyond the screenshot itself (T10's rule applies to both) | TOOL-U-048: labels hold only the ref's digits |
| T13 | A hostile page exploits the browser's renderer (Chromium runs with `--no-sandbox`, as the Workstation's browser already does) | The task's sandbox is the boundary: the exploit lands where the agent's own code already runs, as the sandbox user with no secret, so it gains nothing beyond what any command in the task could do, and the allow-list still bounds its network | SEC-058's probes, which show that nothing in a Desktop sandbox reaches beyond the allow-list |

Residual risks, accepted with the owner's principles: an agent acting in a signed-in account (T7), what a model
provider receives in screenshots (T10, as for any content sent to a remote model), and a renderer exploit inside the
task's own sandbox (T13), which the sandbox and the allow-list bound as they bound the agent's code.

## Contract

### The gateway and sandbox side

- **Template:** `desktop` in the capability list; the Desktop template's resource defaults (section 10).
- **Routes:**
  - `GET /api/v1/conversations/{id}/screen` (WebSocket, session cookie, exact `Origin`): the owner's viewer. Refusals:
    `forbidden` ("conversation ownership is required") for another person's task, as the task routes answer it
    (ADR-0034's S6 conventions); `not_found` for a missing one; `no_desktop` when the conversation's environment has
    no `desktop`; `screen_viewers` past four; `sandbox_not_running` when no desktop sandbox is up (the page offers to
    start it by sending a message, or waits).
  - **How a refusal reaches the page** (amended 5 Oct 2026 with the web side: a browser can't read the status of a
    refused upgrade, which surfaces as close code 1006 with no reason, like a network drop). A request without a
    session or with a wrong `Origin` is refused before the upgrade with plain HTTP `401` or `403`, which the page
    never words. Any other refusal accepts the upgrade and closes at once, before any screen data, with an
    application close code and the error code as the close reason: `4403 forbidden`, `4404 not_found`, and `4409`
    with `no_desktop`, `screen_viewers` or `sandbox_not_running`. `sandbox_not_running` means the runtime's link is
    down, also when an open viewer's sandbox stops (the page waits for the next run). A bridge that drops, or doesn't
    come within 35 s, while the link is up closes `4409 screen_unavailable`, which the page retries with backoff. A
    close with any other code, 1006 included, is a dropped connection, which the page retries. (Amended in slice 5.)
  - Messages on that socket from the page: `{"type": "take_over"}` and `{"type": "hand_back"}` as text frames;
    a second tab's `take_over` while another holds the screen is refused without closing: that tab keeps its picture
    and gets `{"type": "control", "holder": "other"}` again (amended in slice 5).
  - **Who holds the screen** (amended 5 Oct 2026 with the web side, its note's §7 point 4): the gateway's first text
    frame on every viewer socket, and another on every change, is `{"type": "control", "holder": "none" | "you" |
    "other"}`, so a tab knows its own state without remembering it. Take over belongs to the socket, not the tab: when
    the holder's socket closes for any reason (a dropped network included) Take over ends at once and is recorded as
    `screen.handed_back` with `ended: "socket_closed"`. A gateway restart ends it too: at its next start, before it
    listens, the gateway records `screen.handed_back` with `ended: "restart"` and no length (the time held before the
    restart isn't known). A tab that reconnects therefore never holds Take over; it reads
    `holder` (`none`, or `other` if another tab took over meanwhile) and may take over again.
  - `GET /api/v1/conversations/{id}/screenshots/{screenshot}` (session cookie; its §7 point 1): one stored screenshot,
    `screenshot` being the store id that `screen.capture` names. The owner only, as the transcript: `403 forbidden` for
    another person's task, `404` for a missing task or an id this task never stored, `410 screenshot_expired` for one
    the retention purge removed (its event remains, section 9). `200` with `Content-Type: image/png`,
    `Cache-Control: no-store` like every other response (a screenshot cached on a shared computer would outlive the
    task's deletion; at most 512 KiB, a replay fetches it again), `X-Content-Type-Options: nosniff` and
    `Content-Disposition: inline`.
  - `GET /runtime/screen` (WebSocket, the runtime token): the sandbox's bridge, held as a long poll: `101` when a
    viewer is open or opens within 30 s, `204` at 30 s otherwise; one per viewer, at most four per sandbox, the fifth refused `409`; `403` for a
    conversation without `desktop`.
  - `POST /api/v1/conversations/{id}/screen/sign-out` (`{}`, session, exact `Origin`, an `Idempotency-Key` as every
    write): Sign out everywhere; `202` with no body. Refused `403 forbidden` for another person's task, `404` for a
    missing one, `409 no_desktop`, and `409 run_active` while a run is active, so the screen never changes under the
    agent (during Take over there is no run, so it works then) (GW-I-222). It reaches the desktop's browser only, never
    a desktop application's own sign-in in the sandbox's home, and the page words it as the browser's.
    (Amended in slice 5 with the runtime side.) With the task's runtime linked the gateway sends it
    `screen.sign_out` (revision 14), and the runtime closes the browser, empties its profile and answers
    `screen.sign_out.done` (`ok`, or a `reason`: `profile_in_use` or `io_error`, a closed list). On `ok` the gateway
    records `screen.signed_out`; otherwise `screen.sign_out_failed` with `reason` `not_cleared`, or `unconfirmed` when
    no answer arrives within 10 s or the link drops first. A second sign-out while one waits (another tab) joins it:
    `202`, no second ask, and one outcome notice for both. With no runtime linked there is nothing to clear (the
    profile is the sandbox's tmpfs, and a new sandbox starts signed out), so it records `screen.signed_out` at once.
- **The link:** a conversation whose environment declares `desktop` is refused below revision 14 (`runtime_too_old`,
  `needs` 14), as ADR-0026's notes do for teams; the event itself is in the runtime contract.
- **Transcript notices** (the gateway's own, never sequenced into the runtime's log): `screen.taken_over`,
  `screen.handed_back` (who and for how long) and `screen.signed_out`. How they reach the page (amended 5 Oct 2026 with
  the web side, its §7 point 2), as `team.changed` does for a team (ADR-0034):
  - **Stored** by the gateway in its own table, one row each: `{"kind": "screen.taken_over" | "screen.handed_back" |
    "screen.signed_out", "by": {"id", "name"}, "at": "<RFC 3339>", "after_seq": <the conversation's last stored
    event seq when it happened>}` (`name` is the person's sign-in address, at most 254 bytes: the gateway keeps no
    other name for a person; amended in slice 5), and for `screen.handed_back` also `"ended": "hand_back" |
    "socket_closed" | "restart"` and, but for a restart, `"seconds"` (how long Take over lasted). Never what was typed or pasted. `after_seq` places the notice in the
    transcript after that event, so the page interleaves it without the runtime's sequence holding it.
  - **Read** with `GET /api/v1/conversations/{id}/screen/notices` (owner only, refusals as the transcript's): the
    newest 100, oldest first, `{"items": [...]}`; deleting the task deletes them.
  - **Live** as a gateway-to-clients stream frame `{"type": "screen.notice", ...the same fields}` on the
    conversation's stream, sent once when the notice is stored and never sequenced; a page that missed it (a dropped
    stream) reads the route on reconnect, as it does for `team.changed`.
- **Settings** (ADR-0028): `desktop.max_sessions`, `desktop.screenshot_retention_days`.
- **Storage:** the screenshot store and its purge (section 9); the browser profile in `/run/tqa-browser`, the sandbox's own tmpfs space,
  emptied when the run that follows a sign-in ends (section 6).

### The runtime and tools side

**Conventions.** The tools' errors reach the model as `<code>: <message>`, as every built-in tool's do. Schemas are
strict (unknown fields refused). Sizes count UTF-8 bytes; NUL is refused. The runtime registers both tools from the
environment that `session.config` declares, never from what it finds in the image.

**`computer`** (TOOL-U-043): `{"action": ..., <that action's fields>}`.

| `action` | Fields | Offered | Result text (the model sees) |
|---|---|---|---|
| `observe` | none | always | the snapshot in a `source="screen"` untrusted block: `snapshot` number, `source` (`dom` or `a11y`), the window title, then one line per element: `e7 button "Save" (focused)` |
| `screenshot` | none | on a route with `vision` and without `computer_tool` | "Screenshot 1280×800." plus the fresh snapshot as `observe` answers it, and the image marked with its refs' numbers (section 2, section 3) |
| `click`, `double_click` | `ref` (`"e7"`, or a mark's bare number `"7"`), or `x` and `y` | the `ref` form always; `x`/`y` with `pixel_grounding` or `computer_tool` | what changed (section 2), such as "Clicked e7; a dialog opened: 'Save as'." |
| `drag` | `x`, `y`, `to: {x, y}` | with `pixel_grounding` or `computer_tool` | what changed |
| `scroll` | `ref` or `x`/`y`, `direction` (`up`, `down`, `left`, `right`), `amount` (1–20 notches, default 3) | the `ref` form always; `x`/`y` with `pixel_grounding` or `computer_tool` | what changed |
| `type` | `text` (1–4,000 bytes), `ref`? | always | "Typed 42 characters into e3." |
| `key` | `keys` (1–8 keys joined by `+`, xdotool names) | always | "Pressed ctrl+s." plus what changed |
| `wait` | `ms` (100–10,000) | always | "Waited 2 s." plus what changed |

A route with `computer_tool` also gets the provider's tool, with the display's width and height, and its actions run on
the same executor (section 2). The provider tool's results carry the same texts and its screenshots.

**Refusals** (the tool's error codes):

| Code | When | Test |
|---|---|---|
| `invalid_args` | a field breaks its rule, naming it as every built-in tool does (`invalid_args: /ref: ...`; amended in slice 1 from `invalid_request`): a point outside the last screenshot, an unknown key name, `text` too long or empty, a `ref` and `x`/`y` together, or neither where one is needed | TOOL-U-043 |
| `stale_ref` | a `ref` not in the latest snapshot, an element that has gone, or a snapshot ended by a window change; "Observe again." | TOOL-U-044 |
| `no_screenshot_yet` | a coordinate action before any `screenshot` in the run (there's no image to map from) | TOOL-U-043 |
| `screenshot_limit` | past `computer.max_screenshots_per_run`; "Work from observe." | TOOL-U-045 |
| `action_limit` | past `computer.max_actions_per_run` | TOOL-U-045 |
| `display_unavailable` | the display or the accessibility bus isn't answering (a broken session); the run's repeated-failure rule applies | RT-I-046 |
| `action_failed` | the element's own action and the XTEST fallback both failed, with the element and the reason | RT-I-046 |

**The model's input** (CORE-U-081, LLM-U-052): section 3's rules. The images of the last 3 screenshots of the run, each
counted as `IMAGE_TOKENS`. An older screenshot, or one whose local copy is gone (a new sandbox), goes as the fixed line.
The bytes are read from the runtime's local copy when the request is built, and never journaled. The neutral request
puts the image on the tool's result message. Anthropic Messages renders it inside the `tool_result`; OpenAI Chat and
Responses render it as one following user message with only the image and its label.

**The acted-on element: `tool.result`'s `target`** (PROTO-U-037, CORE-U-082; asked for by the web's step lines,
5 Oct 2026). A `computer` call that acted on an element by its ref (`click`, `double_click`, `scroll`, and `type`
with a `ref`) carries that element on its `tool.result` event, so the transcript says "Clicked Save" without
parsing the result's text:
```json
{"type": "tool.result", "call_id": "<the computer call>", "status": "ok", "content": "Clicked e7; ...",
 "target": {"role": "button", "name": "Save"}}
```
- `role` is one of a closed list of 22, so a client words each in its own language: `button`, `link`, `textbox`,
  `searchbox`, `checkbox`, `radio`, `switch`, `combobox`, `listbox`, `option`, `menu`, `menuitem`,
  `menuitemcheckbox`, `menuitemradio`, `tab`, `slider`, `spinbutton`, `treeitem`, `cell`, `icon`, `heading` and
  `other`. Both paths' snapshot roles map onto it (the browser's actionable roles one to one, AT-SPI's as the
  accessibility path words them); any other is `other`. A new value is a protocol revision.
- `name` is the element's name as the snapshot showed it: one line, at most 200 characters (counted in characters,
  as the schema's `maxLength` is; a longer one is cut to 199 and an ellipsis). An element without an accessible
  name falls back to its description: the tooltip on AT-SPI, the accessibility tree's description on the browser
  (its `title` or `aria-describedby`; alt text is already in the computed name). It may still be empty, and a client
  then words the role alone ("Clicked a button").
- Both are screen text, so a client renders them as text only.
- It is absent (never `null`) for `observe`, `key`, `wait`, `type` into the focus, every pixel action and every other
  tool, and on a refusal. The action itself, its keys and its text's length stay in the call's own arguments.
- It is never part of the model's input: the model reads the result's `content` alone, so the history is
  unchanged by it.
- It is a new optional field on an existing event, so it joins revision 14 with `screen.capture`; a runtime below
  14 never writes it.

**The record: `screen.capture`, a new runtime event** (PROTO-U-036). One per screenshot taken for the evidence or for the
model:
```json
{"type": "screen.capture", "call_id": "<the computer call>", "phase": "before", "id": "<store id>",
 "width": 1280, "height": 800, "bytes": 183420, "sha256": "<64 hex>", "sent_to_model": false, "marked": false}
```
- `id` is the store's id, 1 to 128 ASCII letters, digits, `-` and `_`, which a client puts in the read route's path
  as it is. `width` and `height` (1 to 8,192 pixels) let a client reserve the picture's room before it loads.
  `bytes` is 1 to 512 KiB, and `sha256` is 64 lowercase hex digits.
- `marked` is `true` only for the model's own screenshot, and only when at least one element was marked; the evidence
  captures are stored unmarked.
- **Order in the log**, so a replay built from the log alone reads in time order: a `before` capture is journaled
  ahead of its call's `tool.start` (the core gives the tool a pre-start step for it), and an `after` or
  `screenshot` capture ahead of its call's `tool.result`. A client still attaches a capture by `call_id` whichever
  comes first.
- **What was kept:** a `computer` action that takes evidence carries `evidence: {"before": bool, "after": bool}` on
  its `tool.result`, and a `screenshot` call's Ok result carries `evidence: {"after": bool}` (its one capture; `before`
  absent, never `null`), so a client tells "not kept" from "not yet" without reading the result's text. A capture that
  wasn't kept is a present `false`, never an absent field. `evidence` is absent, never `null`, on every other result,
  and like `target` it is never part of the model's input.

**Marks** (TOOL-U-047, TOOL-U-048, RT-I-047): `screenshot` takes a snapshot and marks its boxed refs (mark 7 is `e7`, and
`"7"` is accepted as a ref); a click on a mark acts on that element whatever the image shows at its label; the overlay
draws an outline and a label per boxed element, labels inside the screen and apart, at most 300, scaled after drawing.
The coordinate forms are offered only with the route's `pixel_grounding` (an administrator's route setting, `false` by
default, in the route snapshot from revision 14) or its `computer_tool`.
`phase` is `before`, `after` or `screenshot` (the model's own request). The file goes to the screenshot store first,
over the link's workspace channel (section 9 owns the store and its 512 KiB cap). The event is journaled only once the
store has the file, so an event never names a missing file. A capture that can't be stored is left out, its phase is
`false` in the result's `evidence`, and the action's text says "evidence not kept" for the model; it never fails the
action.

**Protocol revision.** `screen.capture` is a new event a runtime emits, `tool.result`'s `target` and `evidence` new
fields on one, and the route snapshot's `pixel_grounding` a new field the gateway sends, so all are protocol revision 14 (the next after
13's wake marker; ADR-0026). A runtime below 14 never emits it, and an environment with `desktop` is refused at the
link below 14 (the gateway contract above). The floor stays where it is: nothing else changes for other environments.
The screen bridge is its own socket (`/runtime/screen`), and the tools are ordinary tool calls, so neither needs a
revision.

**The screen bridge** (RT-I-045): section 5's rules: dialled once `desktop` is declared and the display is up; held
(`204` at 30 s, then a redial) until a viewer is open (`101`); bytes copied both ways to loopback VNC on its own task, in
chunks of at most 64 KiB; closed by the gateway with the last viewer; reconnected after a drop with backoff from 1 s to
30 s, with jitter; one per session.

**Executing actions** (RT-I-046): XTEST input through `xdotool`; the settle rule (300 ms without change, 3 s at most);
the before and after captures around every acting action; refs from the browser's local debugging connection or from
AT-SPI on the session bus.

### Tests that come first

The runtime and tools side's rows are named with their contract points above: TOOL-U-043 to TOOL-U-045 (TOOL-U-046 retired), RT-I-045,
RT-I-046, CORE-U-081, LLM-U-052 and PROTO-U-036. The sandbox and gateway side:

| ID | What | Contract point |
|---|---|---|
| GW-I-217 | The viewer route: the owner's socket opens; another person's task `403 forbidden`, a missing one `404`, `no_desktop`, `screen_viewers` past four, `sandbox_not_running`; a wrong `Origin` refused | section 5; the routes |
| GW-I-218 | View-only at the gateway: a view-only socket's `KeyEvent`, `PointerEvent` and `ClientCutText` never reach the sandbox, the Take over holder's `ClientCutText` does, an unknown or malformed message closes the socket, `FramebufferUpdateRequest`s paced to 10 a second | sections 5, 7 and 10; T4, T8 |
| GW-I-219 | Take over: the active run is cancelled as Stop; one controller (a second tab told `other`, its socket kept); input passes only from it; Hand back and a closed socket end it with `screen.handed_back`; no queued turn (a message, a team's wake) starts meanwhile | section 6; T5 |
| GW-I-220 | The bridge: the dial held as a long poll (`101` with a viewer, `204` at 30 s), closed with the last viewer, one per sandbox (`409`), `403` without `desktop` | section 5 |
| GW-I-221 | The screenshot store: written before its `screen.capture` is journaled, the 512 KiB cap, the state quota (a full quota keeps no more and the run goes on), the retention purge, deletion with the task | section 9; T10, T11 |
| GW-I-222 | The browser profile in `/run/tqa-browser` (the sandbox's own tmpfs), never the workspace or the state, emptied when the run that follows a sign-in ends; Sign out everywhere and its refusals, `run_active` during a run | section 6; the routes |
| GW-I-223 | The link: an environment declaring `desktop` is refused below revision 14 (`needs` 14), other environments from the floor | the link; revision 14 |
| GW-I-224 | `desktop.max_sessions`: a desktop past the cap is queued with `desktop_sessions`; the two settings' ranges; a task whose environment declares `desktop` gets 500 steps in its run limits | sections 4 and 10; T11 |
| SBX-C-020 | The Desktop template on Docker, and the microVM's refusal in v1.1: the display, the window manager and the VNC server run as the sandbox user and listen on loopback only; its resource defaults within the ceiling, a lower ceiling refused naming the template | sections 1, 10 and 11; T9 |
| SEC-057 | On-screen text that instructs the agent arrives marked untrusted, and the run's egress stays within the allow-list | T1 |
| SEC-058 | SEC-001/006's probes from the Desktop template, with a browser launched without proxy settings | T2 |
| SEC-059 | After the run that follows a sign-in, the next run starts signed out, as do a new sandbox, a share holder's and a fork's desktop; after Sign out everywhere the profile is gone | T6 |
| SEC-005, SEC-001 | Grow a case each: another person's screen (T3); the secret search in a Desktop sandbox (T12) | T3, T12 |

## Slices

Test-first and slice by slice, as agent teams were (DEV-PLAN Phase 10), behind the template: no installation sees a
desktop until an administrator offers a template with `desktop`. Each slice merges into dev with its tests.

1. **The spike: the browser first.** A headed Chromium on Xvfb in a test image, driven by `computer` with DOM refs
   (`observe`, the ref forms of `click`, `type`, `scroll`, plus `key` and `wait`), the settle rule, and the refusals that
   don't need pixels. It measures what the research note left open: memory and CPU per desktop session and the settle
   times, written into section 10's numbers before slice 2 (TOOL-U-043's structure-first rows, TOOL-U-044, RT-I-046's
   browser cases).
   - **As built (slice 1, 5 Oct 2026):**
     - The browser driver speaks the browser's debugging protocol over its loopback port. The runtime starts the
       headed Chromium itself, with its profile in the browser's own tmpfs (`/run/tqa-browser/profile`, section 10),
       a 256 MiB disk cache, and the port read from the profile's `DevToolsActivePort` file. RT-I-046's probe runs
       with exactly a desktop sandbox's mounts: the 16 MiB `noexec` `/tmp`, the 1 GiB shm size and that tmpfs. The image's init starts only the display, and never a browser.
     - Chromium runs without its own sandbox (`--no-sandbox`), as the Workstation's documented browser use does: the
       task's sandbox is the boundary, and Chromium's sandbox needs namespaces it doesn't grant.
     - A click is the element's own `click()`, run on the page's next tick, so a click that opens a JavaScript dialog
       doesn't hold the call. While a dialog is open the page's script is paused: `observe` shows it as its own
       snapshot (its message, OK and Cancel), its state is a new window (so earlier refs are stale), and Return or
       Escape, or a click on OK or Cancel, answers it.
     - In the browser, the fallback is the debugging protocol's input events at the element's centre; XTEST is the
       fallback for other applications from slice 2.
     - The runtime registers nothing yet. The `desktop` capability, and with it revision 14, start in slice 2,
       because a capability in `session.config` is itself a protocol change; `screen.capture` joins revision 14 in
       slice 4, while no release carries it.
     - The test images: `desktop-spike` (the Workstation, Xvfb and its init) and `desktop-probe` (RT-I-046's probe on
       it), both test-only, in images/runtime/Dockerfile. The Desktop template's stage builds on `desktop-spike`.
2. **The desktop template and the accessibility path.** The `desktop` capability, the template image (section 1), AT-SPI
   refs for other applications, the XTEST fallback, `display_unavailable` and `action_failed`; both backends (the
   gateway contract's template and limit rows; RT-I-046's AT-SPI cases).
3. **Pixels and the model's view.** `screenshot`, the coordinate forms and their mapping, images in the request (the
   last 3, the fixed line, `IMAGE_TOKENS`), their rendering on each protocol, and the two per-run caps as administrator
   settings (TOOL-U-043's pixel rows, TOOL-U-045, CORE-U-081, LLM-U-052). With them the marks (section 2): the marked
   screenshot, the bare-number ref, `pixel_grounding` as a route setting and the grounding evaluation's plan
   (TOOL-U-047, TOOL-U-048, RT-I-047).
4. **The record.** `screen.capture` and revision 14, the screenshot store and its retention, the link check for
   desktop environments (PROTO-U-036; the gateway contract's store, retention and link rows).
5. **The Screen panel and Take over.** The runtime's bridge, the gateway's held relay, the noVNC panel view-only, and
   Take over cancelling the run with input handed to the person; the action lines and the replay from
   `screen.capture` (RT-I-045; the gateway contract's relay, viewer and Take over rows; the web rows).
6. **The provider's tool.** `computer_tool` on a route, and the provider's actions on the same executor (TOOL-U-043's
   provider rows). (The suggestion and its card were dropped, 10 Oct 2026; section 8.)
7. **End to end.** A desktop task on the scripted model (structure first, a pixel fallback, Take over for a sign-in,
   the replay after the sandbox is gone), and the security review's tests on the real stack, on both backends.

## Alternatives considered

- **Browser only** (the Workstation's headless Chromium, as today): kept as the spike's first slice, but rejected as the
  whole answer, since it reaches no desktop application and gives the person no live view.
- **Pixels only, as the providers' tools do:** rejected as the default. It needs vision, which shuts out local models;
  it costs a screenshot per step; and it clicks by guesswork where an accessibility action is exact. Kept as the
  fallback.
- **A separate `browser` tool beside `computer`:** rejected. One tool with the observe order covers the browser, and two
  tools would give the model two ways to click the same button.
- **Resending every screenshot in each request:** rejected for cost (section 4's arithmetic).
- **A VNC port the gateway connects to:** rejected. A listening port is one more thing another sandbox could reach; a
  bridge the runtime dials, as it dials the link, keeps the sandbox closed on both backends.
- **KasmVNC:** GPL-2.0, so it can't be bundled in the web client (ADR-0017). **Selkies** (MPL-2.0, WebSocket, H.264) is
  the upgrade path if the spike shows VNC too slow for video-heavy work. **WebRTC-only transports** need media ports and
  relays that conflict with the sealed sandbox network (ARCHITECTURE §4.7's check, 23 Sep 2026).

## The owner's answers (5 Oct 2026)

1. **The network for a desktop:** the same allow-list as other tasks, and no full access. Any site (the **Open web**
   list) stays off unless an administrator turns it on, as for every task (section 7).
2. **Sign-ins by hand:** yes, in Take over, and a signed-in session lasts only for the run that follows it, or until
   the sandbox retires; there is no option to keep one across runs in this version (section 6).
3. **The clipboard:** paste in during Take over only; nothing is ever copied out (section 7).
4. **Steps:** desktop tasks default to 500 steps. The person controls budget and time; the step cap stays, because some
   model servers report no usage (section 4).
5. **Models without vision:** every model may use the desktop, structure first; screenshots are for routes with vision
   (sections 2 and 3).

## Links

- ARCHITECTURE §4.7; DEV-PLAN Phase 11; PRD (computer use items)
- ADR-0012 (agent profiles), ADR-0017 (licences), ADR-0019 (the relay), ADR-0022 (single public origin), ADR-0035
  (network guardrails), ADR-0037 (host keys)
