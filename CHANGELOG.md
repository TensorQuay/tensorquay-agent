# Changelog

Each release of TensorQuay Agent, newest first. A fix release changes no capability, migration or protocol
([DEV-PLAN §7](docs/DEV-PLAN.md)).

## Unreleased

### Changed

- **The MCP client library is rmcp 3.9.0** (it was 3.4.1), past RUSTSEC-2026-0457, -0461 and -0462. None of the
  affected code was built in.
- **Releases are tagged.** Each release is a `vMAJOR.MINOR.PATCH` tag with a page on GitHub's Releases; the README
  and the install guide start from the newest release, and CONTRIBUTING.md says how branches, pull requests and
  releases work.
- **Settings menu (WEB-U-460).** From 768 px the section menu stays in view while a long section scrolls; a menu taller
  than the window scrolls on its own, with the current section in view.
- **Token counts in short form (WEB-U-461).** 2.8M instead of 2 824 643, in each language's own way (282.5万 in
  Chinese); the exact number shows on hover and is what screen readers read.
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
