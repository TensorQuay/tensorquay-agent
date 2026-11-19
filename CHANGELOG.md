# Changelog

Each release of TensorQuay Agent, newest first. A fix release changes no capability, migration or protocol
([DEV-PLAN §7](docs/DEV-PLAN.md)).

## Unreleased

### Added

- **A built web app's own paths work in the preview (GW-U-223, GW-U-224, GW-I-321).** A default Vite or Create
  React App build names its files from the server's root (`/assets/app.js`), which a preview's address can't serve,
  so the page stayed blank. The preview now moves those paths under its own address in the page's HTML and in its
  stylesheets; scripts are never changed. A static export's own pages open by their links (`/about` answers
  `about.html`). Each previewed page carries a small reporter that tells the Files panel what it couldn't load.
- **Administrators see every running sandbox, and can stop an idle one (Settings › Operations › Sandboxes;
  GW-U-216 to 220, SEC-066, WEB-U-486 to 489, WEB-E2E-129).** Each row names its owner, environment, state and when it
  stops, with its CPU and memory against its size; the page shows the slots in use and anyone waiting. Stop now retires
  an idle sandbox the same way its idle window would: the task and its files stay, its next message starts a new one,
  and its owner reads a notice saying an administrator stopped it. A working sandbox is never stopped. Each stop is
  audited. The page shows nothing of a task but its owner and environment. Settings › Status now says whether a sweep
  left anything behind (a container, a network, or a volume in volume mode) and why, and the page's Left behind section
  gives each one's remove command. One database migration.
- **The agent builds web apps that preview (PROMPT-U-028).** Where a web app can be built (Node, with the npm
  registry among the allowed sites), the instructions say the Files panel previews a built folder under a
  sub-path: build with relative paths (Vite `base: './'`, Create React App `"homepage": "."`) and a hash router,
  and say which `index.html` to open. A default build's root paths and a browser-history router show a blank
  frame in the preview. Instruction version 15; anywhere else the instructions are unchanged.
- **A long task can look back past its summary (CTX-U-023, CORE-E-043, E2E-RECALL-001).** When a long task is
  summarised to fit the model's window, the summarised turns are now kept as text in the task's
  `.tqa/<id>/history.md`, and the summary tells the model to search it before answering about anything earlier. Ask
  about a detail from an hour ago and the agent looks it up with its usual file tools, instead of guessing. A task
  that is never summarised is unchanged.

### Changed

- **The rest of the 17.2 design review (18.1; WEB-U-498, WEB-U-499, WEB-U-501 to WEB-U-504, WEB-E2E-136).** A
  Settings form's Advanced and its other disclosures are quiet toggles beside their bold section labels. The usage
  chart is drawn in ink with its busiest day's tokens on its scale, and each share is its percentage on a thin track.
  Status reads as one list, in one text size. Sign-in, setup, joining and the 404 page show the same lockup as the
  app, the mark with "TensorQuay Agent". On a phone a Models card opens its route when tapped. In the dark theme your
  messages have a thin edge, so they stand apart from the page.

- **`edit_file` says what it found, so a model can fix its next call by itself (18.1; TOOL-U-004, CORE-E-041).** An
  `old_text` that matches more than once lists the matching lines with their text (past ten, how many more, counted to a
  thousand); one that matches nowhere names the nearest lines with their numbers and text, never a looser match; the
  same refused call sent a second time also returns the file's lines around them, and a third still stops the run. An
  edit whose `old_text` and `new_text` are the same is answered "no change" and leaves the file as it was; among real
  edits it is skipped and said.
- **`write_file`'s `create_parents` is optional (TOOL-U-003),** read as false when absent.
- **The text tool-call route reads `tool` or `function` for the tool's name and `parameters` for its arguments
  (PROMPT-U-024),** as the native route does; a call block without a name says which keys it had (`no_name`'s new
  optional `keys`, PROTO-U-044; never a value).
- **The task's header is one quiet line (WEB-U-495, WEB-E2E-135).** The title is in regular type at body size, and
  the workspace it is in (and the profile it came from) follows it in grey on the same line, giving way first when
  room is short. A finished task no longer says "Done" there: the end of the transcript and the sidebar say it, and a
  screen reader still hears it. Every other state (Working…, Waiting for you, Failed, Stopped) stays on the line.
- **A file opens beside the chat (WEB-U-494, WEB-E2E-134).** Expand on a file in the Files panel no longer opens a
  dialog over the conversation: the file takes about 60 % of the page beside the chat, which stays readable and usable,
  and a divider between them can be dragged or moved with the arrow keys; each person's choice is remembered in their
  browser. Full width gives the file the whole page (Show chat brings the chat back), and Escape steps back. Every
  kind of file that previews can be opened this way now, and a web page's preview is never loaded twice. On a phone the
  panel takes the screen as before, with a back arrow.
- **The transcript shows no scrollbar until you scroll (WEB-U-485, WEB-E2E-132).** A long task's transcript had the
  browser's scrollbar beside it all the time, taking its width. Now a thin bar shows over the transcript's edge while
  you scroll it, by wheel, touch, keys or Jump to latest, or while the pointer is at that edge, and fades about a
  second after; it never shows while the page follows a running task's new output. It can be dragged, and it takes
  no width, so nothing moves when it shows. The whole main area scrolls the transcript now, its text still the
  centred column: a wheel or trackpad over the side margins scrolls it, and the bar sits at the area's right edge (the
  Files panel's edge when it is open). Keys, focus and screen readers use the transcript as before. With forced
  colours the system's scrollbar stays.

- **For contributors: the web style check keeps type and radii to the tokens (WEB-U-509).** `pnpm tokens` (part of
  `pnpm check`) now also refuses a font size, weight, line height, letter spacing or radius that isn't one of the
  design tokens, and named colours, in CSS, style objects and class names. A style that must stay off the scale says
  why on its line (`token-lint: allow <rule> (<reason>)`) and is listed in `web/scripts/token-allowances.txt`; five
  are, today. Nothing on screen changes.
- **For contributors: every shared component is in the design reference, with its states (WEB-U-510).** `/design`
  (development and test builds) now draws each component in `web/src/ui` from one registry, in both themes, and
  `pnpm check` fails for a new component that has no entry there. Nothing in the product changes.

- **For contributors: a UX pass on every pull request, and a review of what changed at each release (WEB-E2E-142,
  WEB-E2E-143).** Thirty screens at a desktop's and a phone's width, light and dark, must not scroll sideways, must
  draw with the design tokens' sizes, weights and radii (checked in the page itself) and must pass the accessibility
  check. `pnpm ux:compare` lists the screens that changed since the last release for the release review
  (`docs/release.md`).

## 18.0.2 (19 November 2026)

A fix release: a tool call whose number or true/false a model sent as text runs instead of being refused until
the run stopped. No migration, protocol or configuration change: upgrade in place.

### Fixed

- **A page preview's note reads on one line (WEB-E2E-143).** In the Files panel, "Scripts are running in a sandbox"
  was squeezed to a word or two a line beside Stop scripts and Reload; where the row is narrow, the two actions now
  take a line of their own under it.
- **A button's label is never cut (WEB-U-511, WEB-E2E-141).** A label longer than the room it has, such as a long
  translation in a narrow dialog, now wraps onto a second line inside its button instead of running past its edges.
  Checked in all ten languages at a phone's and a desktop's width.

- **A task that may not lead helpers is told why, never "unknown tool" (TOOL-U-147, RT-I-062, GW-I-329,
  WEB-U-497).** A lead whose team had ended, asked to continue, called its old team tools and was answered three
  times that no such tool exists, while the page said "Couldn't start a helper" each time. Now the gateway says why
  a task may not lead (helpers switched off, the task was a helper, its environment too old, or its team has ended),
  the task is answered in one plain sentence that tells it to do the work itself and say so, and the page reads
  "Can't lead helpers here" with the reason.
- **A tool call whose number or true/false arrives as text still runs (TOOL-U-146).** One model server sends an
  optional number, such as the shell's time limit, as `"30000"` instead of `30000`. The tools refused it, and a model
  that couldn't see why sent it again until the run stopped. The agent's own tools now take an exact number, `true`,
  `false` or `null` written as text where the setting takes no text. Anything else is still refused, and the refusal
  now says what was sent (`expected null or integer, got a string`). Tools from a connected server get their
  arguments exactly as the model wrote them, as before. The shell had lost its own argument rules inside the
  runtime's process-cleanup wrapper, so a blank or `null` optional value reached it too; it now has them.

## 18.0.1 (18 November 2026)

A fix release: a PDF read's step names its pages, the Screen panel's ⋯ menu fits its items and stays inside its
dialog, the expanded screen picture keeps its frame on a phone, and a PDF's page pictures fit the Files panel. The web
app moves to Next.js 16.4.5. No migration, protocol or configuration change: upgrade in place with 18.0.1's images.

### Fixed

- **Two details from the 18.0.1 live look (WEB-U-492, WEB-U-493, WEB-E2E-133).** On a phone the Files panel wrote a
  file's size on two lines beside Expand ("661.6 / kB"); the size now reads on one line, and the actions move under it
  when the row can't hold all three. A PDF read's output in its step scrolled sideways, its paragraphs one long line
  each; it now wraps them, a long address or word included, while a code or text file's read keeps its lines.

- **A PDF read's step names the pages it read, not lines (WEB-U-482).** After reading pages 1 and 2 of a PDF, the
  step said "Read uploads/report.pdf lines 1–2"; it now says "pages 1–2", in every language. The result's own page
  marks decide it, so a PDF saved without its extension reads as pages too.
- **The Screen panel's ⋯ menu is wide enough for its items at desktop widths (WEB-U-483, WEB-E2E-128).** At 1280 and
  1440 px the menu opened no wider than the room to its ⋯'s end and cut its words ("Keep the sign-i…"); where that
  room is narrower than the menu, it now takes the panel's width. A phone's panel places it as before. A menu in a
  dialog opened from the panel (the Screen's or a spreadsheet's Expand) now opens under its button: the panel's bounds
  held it to the panel's strip of the page, where the Screen's ⋯ menu had no width at all.
- **A menu's notes wrap at a readable width, and a menu stays inside its dialog (WEB-U-490, WEB-U-483,
  WEB-E2E-130).** The Screen panel's ⋯ menu, opened from its Expand dialog, set each note on one line, 658 px across the
  window, and on a phone ran past the dialog's edges. A menu item's note and its reason now wrap at 24ch (as the menu
  already read on a phone), and a menu opened in a dialog stays inside it, beside its ⋯.
- **The expanded screen's picture keeps its frame at a phone's width (WEB-E2E-131).** Under Take over, the picture in
  the Screen's Expand dialog took the dialog's whole width at 390 px, and its frame, drawn just outside it, was cut on
  both sides; the dialog now leaves the frame its room.
- **A PDF's page pictures fit the Files panel (WEB-U-484, WEB-E2E-128).** They ran about 10 px past the panel's right
  edge: each page was drawn at the panel's width with its border on top, and the column's scrollbar, which arrives
  with the pages, took more. The border is now inside that width, and the scrollbar's room is kept from the start.

### Security

- **The web app is on Next.js 16.4.5** (it was 16.4.1), past two moderate advisories in which a `'use cache'`
  function's entry could serve one root param's content for another: GHSA-mfr8-8qww-v76h (through `unstable_cache()`)
  and GHSA-vc7v-qv4w-wrv8 (nested `'use cache'` functions). The app uses neither `'use cache'` nor root params, so it
  was not affected. Builds no longer stop to recommend an upgrade (`experimental.agentUpgrade` is off, WEB-U-491).

## 18.0.0 (17 November 2026)

A feature release: the agent reads PDFs and pictures, a desktop sign-in can be kept for a whole task, a folder
previews as a real site, administrators see the sites the network refused lately and can allow them, and each release
now ships prebuilt images for amd64 and arm64. Two database migrations and a protocol revision (15); no configuration
key changes.

### Added

- **A workspace folder can be served as a site for the Files panel's preview (18.0).** Most sites are many files, not
  one page: a pass minted for a folder serves its `index.html` and, by relative address, its style sheets, scripts,
  images and fonts, read-only, for 15 minutes from the last use. The page stays sandboxed with an opaque origin; it
  may load from the internet only what the install's network setting allows, which the browser enforces before a
  request leaves. A built site's own routes answer its page. The pass is forgotten at a restart.
- **Settings › Internet access lists the sites refused lately, with Allow this site (GW-I-323, WEB-U-474).** The
  network proxy keeps the last 200 destinations it refused (host, port, why, how often, when last). An administrator
  sees them under Recently refused; where a rule would open the site, Allow this site opens Add a site with its host and
  port (or the host's rule with the port added), the same form and audit as any rule. Nothing waits for it: the task
  has moved on, and its next connection is decided by the list as it is then. A restarted gateway starts the record
  empty; the log keeps the rest.
- **The agent reads PDFs (PRD item 30; ADR-0040; TOOL-U-145, PDF-U-001, PDF-U-002, CORE-U-092).** `read_file` on a PDF answers
  its text page by page, with page numbers, for every model; on a model that reads pictures, the pages it just read are
  shown as pictures too. The reader is built into the product (oxidize-pdf for the text, hayro for the pictures, both
  pure Rust, pinned exactly), runs inside the task's sandbox, and is capped on file size, pages and text. Before, a PDF
  read as "[binary file]" and the agent had to convert it with a shell tool first.
- **A desktop sign-in can be kept for the whole task (ADR-0039 §6, amended; PRD item 24; PROTO-U-045, RT-I-060,
  GW-I-324, WEB-U-475).** On the Screen panel's ⋯ menu, "Keep the sign-in for this task" keeps what you signed into
  during a Take over across the agent's runs, until you turn it off, clear the browser's sign-ins, delete the task, or
  the desktop reaches the installation's time limit; the desktop keeps running meanwhile (it is no longer retired for
  being idle while the switch is on). Each change is noted in the transcript with who made it, and the panel says when
  it ends. Off, as before and by default, every run
  starts signed out. Protocol revision 15 carries it to the runtime; an older runtime keeps the old behaviour.
- **Prebuilt images for each release, for amd64 and arm64 (REL-U-013, REL-I-006).** From 18.0, a release tag builds
  the gateway, web app and Minimal, Workstation and Desktop runtimes natively for both architectures (Apple silicon
  included), checks them, and publishes them on the GitHub Container Registry as
  `ghcr.io/tensorquay/tensorquay-agent-<name>:<version>`. `deploy/pull-images.sh` pulls the release the checkout is
  on, pins each image by its digest, tags the runtimes so the environments register themselves as built ones do, and
  prints the same three lines for the install's step 1 as `deploy/build-images.sh`: a few minutes in place of a build
  of 20 to 45. Building from source stays as it was.

### Changed

- **The task page's first scripts are 14 KiB lighter (WEB-U-472, WEB-U-473).** The code that draws a turn, the copy
  it shows, and the validators of answers the page reads later (a workspace, an upload, a task's key uses, a run's
  changes, a folder's page, the accounts, a desktop's notices) load when they are first needed; a task without a turn
  never downloads them, and a message box given focus loads them ahead of the first turn. The page's first load is
  225 KiB gzip of JavaScript, 15 KiB under the 240 KiB line it is held to, where 17.4.1 had 1 KiB of room; nothing on
  the page changes.

### Fixed

- **The Screen panel's picture no longer shows a dark box before the desktop's first frame (WEB-U-477).** For about a
  second after the panel connects to the desktop (as when a task's page opens), the picture was the screen client's
  own dark backdrop, not the desktop, whose bare root is white by design; it now shows the panel's surface until the
  first frame arrives.
- **Clear the browser's sign-ins stays offered while a task keeps its sign-in (WEB-U-476).** With the switch on, a run's
  end keeps the desktop browser's sign-in, but the Screen panel's menu said "The browser is already signed out: each run
  ends that way" and refused the clear once a run had ended after the Take over. The panel now reads the switch's own
  notices: a sign-in may remain past a run's end while the task kept it at that end, and the switch turned off ends it.
- **A desktop browser that died is started again (RT-I-061).** The runtime kept the session's browser once started and
  never looked at it again: after a crash, a kill or an out-of-memory end, the next run sent its keys to no window.
  Each run's start and each reset now see that the browser's process has exited and launch it again, as ADR-0039
  section 6 says. The launch reads only the new browser's port file: a killed browser leaves its own behind, and
  reading it first made the relaunch fail at once (TOOL-U-043, WEB-E2E-125). Before, in 17.4 a run that started after
  the browser died had none until its end started one; with a kept sign-in (new in 18.0) no run's end did, so the
  browser stayed gone.
- **A task deleted while the container engine fails keeps nothing behind (GW-I-325).** If the engine refused the
  sandbox's removal at the delete and again on the failure path, nothing tried again until the gateway restarted, so
  the deleted task's sandbox, screenshots and browser profile stayed. The removal is now retried in the background,
  and the task's files go once it succeeds. Found by the 24-hour soak.
- **A long name is never cut without a sign (WEB-E2E-124).** A name longer than its space wraps whole, a long word
  broken anywhere: a dialog's title, a channel's name, members, posts and task board. Where a row is one line by
  design (a channel's heading, a file's row in the Files panel, a picker's choice), the name ends in an ellipsis and
  the whole name is its tooltip and accessible name. Before, some were cut with no sign, and a channel page or the
  share dialog could scroll sideways.
- **Caps Lock types capitals on the Desktop under Take over (RT-I-059).** With Caps Lock on, a Mac viewer's letters
  came out lower case: the screen sharer added Shift for each capital while the desktop's own Caps Lock was also on,
  and the two cancelled. A viewer's Caps Lock no longer toggles the desktop's; letters arrive as typed, and Ctrl+A and
  other layouts' symbols are unchanged.
- **Only the sidebar's task list scrolls (WEB-E2E-123).** With a long task list the whole sidebar scrolled, taking New
  task, Workspaces, Channels and Search out of view. Now the brand row and those links stay at the top, the account
  row at the bottom, Search at the top of the list, and only the tasks scroll between them.
- **The agent can look at a picture in its workspace (CORE-U-090, TOOL-U-143).** When the agent reads a picture
  file with `read_file` (PNG, JPEG, GIF or WebP, recognised by its content), a model that reads pictures is shown
  the picture itself: the latest three read in a run, within the request's room. A model that doesn't read pictures
  is told so in plain words. Before, the agent got only "[binary file]", so it could convert a photo to JPEG and
  still tell the person it couldn't see it.
- **An attached iPhone HEIC photo tells the agent how to see it (CORE-U-091, TOOL-U-144).** No model reads HEIC, and
  the product ships no HEIC decoder: the agent is told the file is a HEIC photo, to convert it to JPEG or PNG in its
  workspace with a tool it can install or write, and to read the converted file, which it then sees; when it can't
  (no network for a tool), it asks the person for a JPEG or PNG. Before, it got a bare path and no hint.
- **A message whose environment couldn't start is kept, and the page says so.** A first start that failed (the
  sandbox could not be created, the runtime exited before it registered, or the model's decision engine was gone)
  told only the open stream; after a reload the message was gone without a trace. The task's queued list now keeps
  it as dropped with the start's reason, the stream hears the queue change, and the composer reads "Not sent,
  because the environment couldn't start" with Send again and Dismiss, as for a run that failed.
- **Desktop menus and tool bars at the top of a window are marked again.** Since 17.1, an element whose box lay
  outside the focused window's frame got no mark; the frame's position came from a tool that, under the desktop's
  window manager, placed it about 20 pixels too low, so the menu bar and the tool bar of an application such as
  Mousepad or LibreOffice had no marks and the agent could not click them by mark. The frame is now the window's
  real client area.
- **A model that can't be reached says so, and a task on it stops waiting (GW-I-320, WEB-U-469).** When a call to a
  model's server can't connect (the server stopped, or its address is gone), the model menu reads "Not answering
  right now" on that model and New task starts on another model. A server that stays down for more than 30 seconds
  is answered for at once, with one real try every 10 seconds, so a task on it fails quickly instead of each try
  waiting for the connection; a server down only briefly (a restart) is still tried on every call, so a task's own
  retries reach it when it's back. Before, a stopped model looked available and a message on it showed "Sending…"
  for 20 seconds before failing.
- **A task's page catches up when its live updates are held back.** Through a proxy that buffers the event stream,
  the task's header could read Done while the transcript stayed on "Working…" with a Stop button, and nothing let
  you send. The page now notices a run the task's record says has ended, or a stream that never opens, reads the
  missing events itself and reopens the stream from where it got to; the Send button follows the record, so you can
  write while the page catches up. Silence alone changes nothing: a long quiet step stays a working run.
- **Deleting a team's lead task with its helpers now removes the helpers' own copies and the team's messages
  (GW-I-108).** Each helper's copy of the lead's files is deleted with the helper's task, whether the helper is
  deleted on its own or with its lead, and the purge removes it within minutes; a copy you started another task in
  stays yours. The team's message log goes when the lead's task is purged. Before, in 17.4.1 and earlier, both
  stayed for good: the copies, still listed among the person's workspaces, with their files on disk, and every
  message the team had sent. Deleting the lead's workspace with its tasks is refused while the lead has helpers
  (`409 has_helpers`), as deleting the lead alone is, and the web app says what to delete first; before, the
  workspace's tasks went in bulk and the team kept running for a deleted lead.

### Known limits

- **A browser that dies while no run is going stays gone until the next run starts or the next reset (RT-I-061).** A
  Take over in between finds the desktop without the runtime's browser; a message to the task starts a run, which
  starts it again.
- **A changed Desktop limit applies to desktops that start after the change.** The screenshot and action caps go to
  a desktop when its task's environment starts, and a desktop already running keeps the caps it started with, as the
  Limits page says beside each limit. To apply a new cap to a running task, stop the task and start it again.
- **Under Take over, a key sent within a few milliseconds of a click can be lost** while the desktop's browser moves
  its focus. Type at a person's pace, or click once more and send the key again. The agent's own actions are not
  affected: each one waits for the screen to settle first.
- **The PDF reader's misses are its engines' (ADR-0040).** A form's field values, and text in a font with no
  ToUnicode map, read wrong or empty; a model that reads pictures sees the page as it looks.
- **On the Firecracker backend, a task's disk is slower than the host's (PERF-012).** Inside a microVM,
  sequential writes run at about 55 to 60 % of the host disk's speed, random writes at about 30 to 40 % and reads at
  60 to 85 %, as in every release since 17.0. On the Docker backend, the default, each is 93 % or more.
- **In Safari, a previewed page whose script keeps the processor busy stops the app responding until the script
  ends.** Chromium-based browsers (Chrome, Edge) run the preview apart and stay responsive, as WEB-E2E-122 checks;
  WebKit runs it on the page's own thread (8 s in the test's busy page). Close the preview or use **Stop scripts**
  once the page lets go. A Safari-safe way is planned for 18.1.
- **Three small display issues, fixed in 18.0.1.** A PDF read's step names the pages it read as lines ("lines 1–2"
  for pages 1 and 2); the Screen panel's ⋯ menu cuts its descriptions short at desktop widths, though each item works
  as described; and a PDF's page pictures in the Files panel run a little past the panel's right edge.
- **A lead task leads one team (experimental agent teams).** Once its team has stopped, a follow-up to the lead task
  is done without helpers. The lead may still try to start helpers it used before, and each try shows as "Couldn't
  start a helper". To take a helper's part further, write to that helper, which keeps its work and files; for a new
  round, ask a new task to lead ([Helpers](docs/user-guide/helpers.md#stop)). Planned for 18.1: rounds.

### Upgrade notes

- **Two migrations, applied by the gateway at its start; back up the database first as usual.** 0104 adds the task's
  `keep_sign_in` switch (off for every existing task), 0105 widens the screen notices' kinds for the switch's two
  notices. Neither removes data (GW-I-324; the upgrade fixture moves to 105).
- **Protocol revision 15: rebuild or pull every runtime image (PROTO-U-045, PROTO-U-046).** The Minimal, Workstation
  and Desktop runtimes carry the new revision in their labels; a 17.4 runtime still works against the 18.0 gateway but
  keeps the old behaviour (every run's end empties the browser's profile, and the kept-sign-in switch has no effect on
  it). Pull with `deploy/pull-images.sh` or build with `deploy/build-images.sh` from the v18.0.0 tag, then start the
  gateway and web with `--force-recreate`, as the install guide says.
- **No `gateway.toml` key is added, renamed or removed** (`deploy/examples/gateway.toml` is unchanged since 17.4.1),
  and no compose file, port or environment variable of the stack changes. The install guide's step 0 now offers the
  pull in place of the build.
- **The kept sign-in and the desktop's idle retirement:** with the switch on, a task's desktop is no longer retired for
  being idle; it ends at the installation's desktop time limit, as before for a busy one (RT-I-060, GW-I-324). Nothing
  to configure; worth knowing for a host that counts on idle desktops giving way.
- **The network proxy's Recently refused record is in memory (GW-I-323):** empty after a gateway restart, the log keeps
  the history. No retention setting.

## 17.4.1 (12 November 2026)

A fix release of the web app, the image builds and the install guide, and the system-call library past one
advisory. No migration, no protocol change.

### Fixed

- **A long name or host no longer pushes a Settings table's actions off the screen (WEB-E2E-120).** A key's name, a
  site's host, an email or a display name longer than its column wraps inside it, and its row grows a line. Beside a
  key's note, its words are cut first and its link wraps second, so an everyday key's name stays on one line and its
  address whole at 1024 px and wider. Before, in 17.4.0 too, the table scrolled sideways and the row's Edit and ⋯ left
  the screen.
- **A Settings table that doesn't fit its area shows its rows as cards (WEB-E2E-119).** Beside the expanded sidebar
  from 1025 px, and in windows of about 920 to 1019 px with the rail, some tables needed more room than their area
  had: Decision engines, Environments and API tokens scrolled sideways and hid a row's actions, or squeezed an
  everyday name onto two lines. Each table now measures itself, as it loads and whenever its area or its content
  changes, and shows its rows as cards where it doesn't fit and as a table where it does. Before, in 17.4.0 too, only
  an area under 600 px showed cards.
- **A page preview says when the page needs the internet (WEB-U-467).** Previews run offline, so a page that loads a
  library from a CDN or live data from an API showed "Loading…" or an empty view with no word why. The panel now says
  so in a line under the preview's bar, naming up to three of the hosts the page loads from (then "and N more") and
  suggesting the download; a self-contained page looks as before. The preview stays offline.
- **Image downloads survive a dropped connection (REL-U-012).** When the images are built, every download a
  checksum guards (Go, npm, the guest kernel and Firecracker) is tried again after a network error, a TLS drop
  included. A slow but steady link finishes however long it takes; a try ends only when it stalls or crawls, and a
  lasting failure names the URL.
- **Engine budgets read to the nearest (WEB-U-461).** Both of a budget's figures round to the nearest, so a limit
  always reads the same; only when a use would read as its limit does it move to the next figure: "29.9M of 30M"
  with headroom left, "30.1M of 30M" just over, and "29.9M of 29.9M" exactly at it. The budget form's summary
  restates the numbers as typed ("30 000 000 tokens").
- **The task summary's exact token counts (WEB-U-461).** Its tooltip gives the token counts exactly ("218 469
  input"), as a short count's tooltip does, and screen readers now get its details too.
- **The Settings menu holds still (WEB-U-460).** It shows the current section before the page is first drawn, and
  again when a wider window brings the menu back or the fonts finish loading.
- **Usage counts requests one way (WEB-U-461).** Each day's requests read exactly, as the period's summary does
  ("1 234 requests"); token counts stay short.
- **A sent picture uses the transcript's whole width (WEB-U-464).** On a narrow screen it no longer leaves room for a
  scrollbar a phone doesn't draw, and in a very narrow column it keeps 48 px instead of vanishing.
- **The install guide gets a new user through on Linux and rootless Docker (deploy/README.md).** It says how a model
  server on the host is reached from a container on Linux (no `host.docker.internal`, and not on `127.0.0.1`); step 1
  sets the rootless daemon's socket and group itself; it warns that a build moves the image tags every installation on
  the host follows, and how to pin an installation's images by ID (`[runtime.builtin_images]` included) and keep them
  with tags of its own, since with Docker's containerd image store a build deletes the images it replaces; how to
  start a new build's images and check them; how to see which image an environment runs; the sandbox pool check takes
  the pool as an argument; and the storage root's own filesystem is a recommendation, with the reason.

### Security

- **The system-call library is rustix 1.1.6** (it was 1.1.5), past RUSTSEC-2026-0486. Its one affected call here,
  the sandbox's read-back of a file ACL, always passes a non-empty buffer, so the unsound zero-length case was not
  reached.

### Upgrade notes

- No migration, no protocol change. The web, gateway and runtime images are new; build them from the v17.4.1 tag
  and start the gateway and web with `--force-recreate`, as the install guide now says. A browser that holds an old page reloads it on the next navigation.

## 17.4.0 (10 November 2026)

An improvement release of the web app, and the runtime's MCP client library past three advisories. No migration, no
protocol change.

### Changed

- **The MCP client library is rmcp 3.9.0** (it was 3.4.1), past RUSTSEC-2026-0457, -0461 and -0462. None of the
  affected code was built in.
- **Releases are tagged.** Each release is a `vMAJOR.MINOR.PATCH` tag with a page on GitHub's Releases; the README
  and the install guide start from the newest release, and CONTRIBUTING.md says how branches, pull requests and
  releases work.
- **Settings menu (WEB-U-460).** From 768 px the section menu stays in view while a long section scrolls; a menu taller
  than the window scrolls on its own, with the current section in view.
- **Token counts in short form (WEB-U-461).** 2.8M instead of 2 824 643, in each language's own way (282.5万 in
  Chinese). On the Usage page and in an engine's budget, the exact number shows on hover and is what screen readers
  read.
- **A folded group shows the step it is on (WEB-U-462).** While a task runs, a folded activity group is headed by its
  live line (the step, a soft shimmer and the time) instead of a static "Working…".
- **The account menu (WEB-U-463).** Your email and role, then Settings, Usage (administrators), Language, Help, Send
  feedback, API tokens and About, each with its icon. Theme moved to Settings › Account.
- **Sent images show as pictures (WEB-U-464).** Under your message, an image you attached shows as its picture and
  any file opens its preview in the Files panel with one click.
- **The task list's ⋯ is part of its row (WEB-U-449, WEB-E2E-114).** It shows inside the row's hover backdrop at the
  row's end, square and evenly inset, its corners concentric with the row's, with no fill of its own; its own hover
  is a quiet tint. A long title fades out under it instead of being cut, and the row's marks give way while it
  shows. It shows on hover, on keyboard focus and while its menu is open, never because a click left focus behind,
  so one row at a time shows it. On a phone it always shows, calm, and titles end before it. A row's title no longer
  underlines on hover.
- **A lead task's row reads as one row (WEB-U-450).** Its chevron sits on the sidebar's left edge with its title just
  after it; its helpers start where its title starts; an open lead's ⋯ stays on its own line.
- **The whole app is one type step bigger (WEB-U-451).** Body text is 16 px on a 24 px line, small text 14, the
  sidebar, the Settings menu and the menus 15, labels 13, page headings 22 and a task's title 18. Controls are at
  least 32 px high and list and menu rows 36 px (44 px on touch screens, as before). Every size comes from one set
  of tokens. The Settings column is 800 px wide (it was 760) and menus up to 240 px, so they hold what they held; a
  table's controls stay 32 px and its two-line rows are 68 px.
- **The sidebar's top block, and one left edge (WEB-U-452, WEB-U-453, WEB-E2E-115).** New task, Workspaces and
  Channels sit together at the top, then the tasks; the account alone at the foot. The logo, the icons, the group
  labels and the task titles share one left edge, and the spacing is even. On the rail the logo sits on the icons'
  axis and each icon shows its name in a tooltip; no item whose name shows repeats it in a browser tooltip. The
  phone's drawer follows: the same order, its rows inset as the sidebar's.
- **One tab style everywhere (WEB-U-454, WEB-U-455, WEB-E2E-116).** The Files panel's tabs, a channel's To-dos and
  Members, and Keys' Mine and Shared look and work the same: the chosen tab is a soft filled pill, the others plain
  labels that tint under the pointer, with no underline and no bold. The arrow keys move between the tabs and choose,
  Home and End go to the first and last, and only the chosen tab is in the Tab order. Tabs wider than a phone's screen
  scroll sideways in their own row.
- **The task header uses the full width (WEB-U-456, WEB-E2E-117).** A task's header runs across the main area: its
  title and state at the start, the model and environment pickers, Files and ⋯ at the end, while the conversation keeps
  its centred column; with the Files panel open it ends where the panel starts. A long title gives way first, then the
  pickers show their icons, and nothing wraps. New task's header and a channel's header span the page the same way. On
  a phone nothing changes.
- **A compact composer (WEB-U-457, WEB-U-458, WEB-E2E-118).** At rest the message box is one row, about 52 px instead of
  122: + at the start, your text, then Send (or Stop while a run goes) at the end. It grows with your text, line by
  line, above + and Send, which stay where they are; files attached and messages waiting for the agent show as chips
  above the text, only while there are any. Its focus is a thin border in the focus colour instead of a 2 px ring. New
  task's box starts the same, at two lines while its placeholder shows the keyboard hint.

### Fixed

- **A sent picture's frame hugs the picture (WEB-U-464).** A wide picture no longer has an empty band beside it, and
  on the narrowest screens a picture shrinks to fit its message.
- **The Settings menu and engine budgets (WEB-U-460, WEB-U-461).** Only the menu scrolls to show the current section,
  never the page, an administrator's section included; a budget's short figures never misstate where it stands
  ("29.9M of 30M tokens" with headroom left, "30M of 29.9M" when spent), and its tooltip and screen readers get the
  exact figures.

### Upgrade notes

- No migration, no protocol change. The web and runtime images are new (the runtime for its MCP client library); a
  browser that holds an old task page reloads it on the next navigation.

## 17.3 (8 November 2026)

A fix release of the web app: no migration, no protocol change, no gateway or runtime change.

### Changed

- **The composer (WEB-U-427, WEB-U-428, WEB-U-431, WEB-U-020).** Its two buttons share one round icon style, and
  Send is filled only while there is something to send. The box is two lines when empty and grows to eight; the
  keyboard hint sits in the placeholder until the first message. An image or a file on the clipboard pasted into the
  box attaches at once (an unnamed image as "pasted-image-N.png"), as a drop does; pasted text stays text. While no
  sites are allowed, one quiet line under the box says so, with Allow sites for an administrator. Suggestions are
  chips of one style that wrap.
- **Live activity (WEB-U-436 to WEB-U-441, WEB-E2E-103 to WEB-E2E-106).** While the agent works, the last line names
  the step ("Reading README.md…", "Running the tests… · 12 s") with a soft light across its words. Each tool call is
  its own line from the moment it starts, with a small turning mark, and settles into its done line with how long it
  took; a running command's output grows in its card. The model's thinking streams in an open panel under the line
  and folds to "Thought for 12 s" once the answer starts; the caret follows the last word and blinks. One calm mark
  beside the task's title, in its sidebar row and in the tab title says a run is under way.
- **Settings read the same everywhere (WEB-E2E-107 to WEB-E2E-113, WEB-U-442 to WEB-U-447).** Every section opens
  with its name, one line saying what it is and its primary action. Lists have one Edit control, never an underlined
  link; green is only for a check that passed, a plain state is a muted pill. A table's cells are two lines at most on
  even rows: a long line is cut with "…", whole in its tooltip and on the item's own page (an environment's check, a
  route's key, a limit's note); numbers sit on the right; the name column takes the room. Keys has one primary Add;
  a route's source sits on its own line. The sidebar's Show more is a small button, and the open task, the open
  helper and the active Settings item share one selected fill. The workspace filter is a picker on a phone.
- **The pickers and menus (WEB-E2E-102 and the 17.3 rows for the run picker, menus, the route key field, the task
  menu and check rows).** The run picker, the collapsed sidebar rail, menu heights on short screens, the route's key
  field, the task menu's "Keep environment on" note and every settings form's check rows follow the guidelines.
- **UI-GUIDELINES** state the above (§5b, §5c, §7, §8) and the weights the app ships (400 and 700).

### Fixed

- **A click right after a save reaches the next page (WEB-U-105).** A settings page's leave guard could outlive its
  page for a moment after a save and swallow the first click on the list it returned to.

### Upgrade notes

- No migration, no protocol change. The web app's scripts are new; a browser that holds an old task page reloads it
  on the next navigation.

## 17.2 (8 November 2026)

A fix release: no migration, no protocol change.

### Fixed

- **A slow local model passes its check (GW-U-092).** A local route whose model passed the check's tool call and
  round trip, and was still answering when the check's own time ran out in its last step, failed with "timeout". It
  now passes, with "Verified, but slowly" and the step it didn't finish named (`line_breaks_unverified`). A model that
  goes silent past the relay's idle limit still fails, in any step, as its tasks would; running out of time before
  the round trip still fails; hosted routes keep their minute.
- **Applications opened from a Desktop session's shell reach the display (TOOL-U-141).** The shell cleared its
  commands' environment, so the desktop's `DISPLAY` and session bus never reached them and an application failed with
  "cannot open display". A Desktop session's commands now get both; other shells never do.
- **Adaptive thinking is shown, summarized (LLM-U-055, LLM-U-057).** On the newer Messages models the thinking text
  is omitted unless asked for, so the task page showed "Thinking…" with nothing to read. A route with adaptive
  thinking now asks for summaries, and the page's live thinking shows what the model works through.
- **The channel page says which agents aren't running (WEB-U-426).** An agent reads a channel only while its task
  runs. Above the message box, the page now counts the agents that aren't running, says to open each one's task and
  ask it to work in the channel (or to ask its owner, for another person's agent), and lists them with their tasks
  linked. English and Simplified Chinese; the user guide says the same.

### Upgrade notes

- No migration. A route's check verdict is read again at its next check.

## 17.1 (7 November 2026)

A fix release: no migration, no protocol change.

### Fixed

- **Adaptive thinking on Messages routes (LLM-U-055, LLM-U-057).** Newer Messages models refuse a thinking budget.
  A Messages route whose reasoning policy has no `budget_tokens` now sends adaptive thinking with the request's
  effort, and refuses a temperature. Such a route sends no earlier thinking blocks back: the protocol binds each
  block to the conversation before it, and the agent's history changes between steps. Routes with budgets are
  unchanged.
- **The conversation is cached, not only its prefix (LLM-U-056).** With a Messages route's cache hint set, the latest
  message carries a cache breakpoint too.
- **A stalled accessibility tree never holds the screen (TOOL-U-138).** Each tree call is bounded (3 s; a whole
  window's walk 10 s). Past the bound the screenshot is still sent, without that application's elements.
- **No marks where the tree's positions are wrong (TOOL-U-139).** An element whose box lies outside the focused
  window's frame gets no mark; it stays listed for its accessibility actions.
- **Typed line breaks and tabs are keys (TOOL-U-140).** Text typed into a desktop application goes in runs with real
  Return and Tab keys between them.
- **Spreadsheet reading uses calamine 0.38.0** (RUSTSEC-2026-0436).

### Upgrade notes

- No migration. A route saved with adaptive thinking can't be read by 17.0: a gateway taken back to 17.0 refuses to
  start while such a route is stored. Delete those routes before going back.

## 17.0 (3 November 2026)

The initial release: an on-premise AI agent platform with sandboxed agents on Docker or Firecracker microVMs,
a web UI, a desktop environment for computer use, and channels where agents and people work together. What it does
and how to install it is in the [README](README.md).
