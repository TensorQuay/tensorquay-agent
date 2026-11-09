# TensorQuay Agent: UI Guidelines

Status: **draft v0.2 for Product Owner review** (23 Sep 2026; v0.2 adds the quality bar and §12–§17 after the
Product Owner's first hands-on session). One visual system for every page of the web UI:
brand, colour, type, spacing, layout, components, states, motion, accessibility and copy. Tokens are the contract;
components use tokens only. Assets are in [`brand/`](../brand/README.md). Engineering rules for the web are in
[ENGINEERING-GUARDRAILS §5](ENGINEERING-GUARDRAILS.md#5-typescript-react-and-nextjs).

## The TensorQuay UI standard (29 Sep 2026)
The reference for every screen, agreed with the owner: one unified system in the style of ChatGPT and Claude. Where a
section below says otherwise, this section wins; a change to the UI changes this section first, then every place that
uses it, in the same piece of work.

**Principles**
1. **One system, not screens.** Every screen is built from the same few shared components (Popover, Menu, Dialog, Side
   sheet, Page, Row, Field). Changing one changes it everywhere; a screen never styles its own copy.
2. **The work is the page.** The conversation and its result take the centre column. Chrome (headers, toolbars,
   labels) is quiet and small, and never competes with it.
3. **Controls appear where you need them and leave on their own.** Choosing applies at once. Esc or a click elsewhere
   closes. No "Done", no "Save title", no control opened on top of another control.
4. **Say it plainly, once.** No raw codes, ids, paths or engine words on a page. Every failure says what happened and
   what to do next.
5. **Same thing, same place, same look.** One way to rename, to pick, to confirm, to go back.

**The app shell and the task page.** Three columns at most. The sidebar collapses to a rail at 1024 px and to a drawer
on phones; the files panel docks beside the conversation from 1280 px and floats below that. The header holds the
title (renamed in place), the status, the task's model and environment as two small pickers, a quiet Files icon and one
⋯ menu, on one line. Where it is narrow (a phone, or beside the files panel) the pickers show as their icons, never a
cut name; the title and the status give way first, never the controls.

**The composer.** One rounded input at the foot of the conversation. Inside it, bottom row: "+" on the left (attach;
on New task also Workspace… and Agent profile…, which add chips above the text); Send (Stop while working) on the
right. The choices that matter (the model, "Local model ▾"; the environment, "Workstation ▾") are small text buttons in
the header, right of the title and status (New task: its top bar). Each opens a popover below it: a short list with a
check on the current one, one muted line per item, a search field when the list passes 7; choosing applies and closes.
Attachments are chips above the text (name, size, ×; too large is refused on the chip). An empty task page shows one
line of greeting, the composer in the middle and three suggestions; it moves to the foot once the task starts. Enter
sends, Shift+Enter makes a new line, Esc stops editing a message; the hint shows once, then hides.

**Helpers (v1.1).** A task can lead helpers (ADR-0034). A person starts a team by asking in words ("get two helpers
to…"); there is no button or form for it. The task list groups helpers under their lead, the lead's page gets a team
strip, and a helper's page is a normal task page with its brief at the top. "Waiting for you" is a status
like any other, and Stop team is in the lead's ⋯ menu. The details are in §13b. Goals across several teams (later,
ADR-0038) are §13c. The words are "helper", "lead task" and
"team"; never "member", "agent team", "role" or any internal name.

**Which control when.** Pick the lightest control that does the job; a screen that needs something else changes this
table first.

| Control | Use it for | How it opens and closes | Examples |
|---|---|---|---|
| Menu | Actions on one thing | From a ⋯ or a right-click; closes after a choice, on Esc or a click elsewhere | Rename, Share, Delete on a task row |
| Popover | Choosing a value | Anchored under its trigger (above it at the page's foot), inside the window and, opened in a panel (the Files panel), inside the panel's content, never over the page beside it; the Changes panel's runs read one line each (the first message cut by an ellipsis, the time muted), Latest on the trigger only; never past the window: no taller than the room on its side of the trigger less 16 px, a longer list scrolling inside it, to its last entry, under its Find box, which stays at the top; choosing applies and closes; Esc or a click elsewhere closes; focus returns to the trigger; arrows move between rows | Model, environment, team, theme |
| In-place edit | Changing one short text | The text itself becomes the field; Enter or a click elsewhere saves, Esc cancels | A task title, a workspace name |
| Dialog | Confirming, or 1–3 fields | Centred, with its buttons; Esc cancels | Delete this task?, Invite a person, New API token |
| Side sheet | Looking at one item without leaving the list | Slides in from the right; Esc or × closes; no long forms | A key's history, a person's activity |
| Page | Creating or editing anything with sections | Its own address; "← Back" at the top; Save and Cancel pinned at the bottom; asks before leaving unsaved changes | Environment, model, agent profile, connector, team |

Never: a control opened on top of another control, a "Done" button to close a picker, or a long form in a dialog.

**Settings.** From 768 px the section menu stays in view while a long section scrolls; a menu taller than the window scrolls on its own, the current section kept in view (WEB-U-460). Each section opens the same way: its name as the heading (22/30), one muted line saying what it is,
then its primary action (People invites after its list, WEB-U-156); a view of it, such as the shared keys, opens the same,
and a page inside it has its own title under its way back instead. Each section is a list first: one row per item (name,
one muted line, status badges, Edit and ⋯; Edit is one small ghost button, the same whether it opens a page or a
dialog, never an underlined link). Add
and Edit open a page (`/settings/<section>/new`, `/settings/<section>/<name>`), never a dialog. The page frame: a "←
<Section>" link at the top, the title, sections with headings, in one column at every width, as ChatGPT's and Claude's
settings: each field's label above its control, fields at most 640 px wide (`--field-width`); Save and Cancel pinned
flush on the view's bottom edge (nothing of the page shows below them) with "Unsaved changes" when there are some, and
an unsaved-changes guard on every way out; Back and deep links work. Where a person would otherwise type something
technical (an image id, a route address), the page offers choices first and keeps typing under Advanced. A new item's
ID is made from its display name, which comes first: a quiet line "ID: data-science · Change" shows it, and Change
opens the field; the ID is fixed once the item exists. A save that a form refuses is never silent: the first field to
fix takes the focus and scrolls into view, and a line by Save says "Fix 1 field above" (or how many), on every
settings form, page or dialog.

**Long lists use the full width** (added 30 Sep 2026 after the owner's review of the Agent profile page). A settings
page has no label column: short fields sit under their labels, at most 640 px wide. A section that is a list of
choices (tools, tool servers, skills, connectors) spans the whole content width as a grid of small group cards, about
three per row at 1280 px and one on phones. Each item is one line (checkbox and name) with its description as a muted
second line; a select-all control sits above the grid. A list never runs as one narrow column beside an empty label
column.

**Type, spacing, colour and motion** (tokens only; no screen uses its own numbers; one step bigger since 9 Nov 2026,
§4). Body text 16 px on a 24 px line, navigation and menus 15 px on 22, small text 14 px, 13 px muted labels (sentence
case, never monospace), page title 22 px, task title 18 px weight 700. Controls at least 32 px high; navigation and menu
rows 36 px (44 px on touch), table rows 40 px (68 px in a table with a two-line cell), spacing steps 4, 8, 12, 16, 24.
Conversation 720 px at most, centred; Settings content 800 px at most; sidebar 260 px, rail 56 px. 8 px corners (a
control inside a row, such as its ⋯, the row's less the inset, so the two are concentric), 1 px quiet borders, the
composer 16 px.
Neutral greys for chrome, one accent for the current choice and the primary action; green, amber and red only for a
stated meaning (verified, warning, failed). One line-icon set, 16 px in rows and 18 px in the composer, before every
menu and navigation item. 120–160 ms fades and slides for popovers, sheets and the files panel; nothing moves on its
own; reduced motion respected. One focus ring on every control (fields, buttons, rows): a 2 px blue ring (`--focus`, its
own colour, never the accent) at a 2 px offset, one shared rule; danger is a crimson red, clearly apart from the
orange accent in both themes. Every token has a dark value.

**How a screen is checked before it counts as done:** it uses only the shared parts and tokens; 1280 and 390, light
and dark screenshots are reviewed against this standard and side by side with ChatGPT and Claude; keyboard only, every
control is reachable and Esc and Enter behave as the table says; every failure on it has plain words and a test; a
real first-hour session before any push to main.

## 0. The quality bar
The web UI is the product's only human surface, so it is held to the standard of the best agent and developer tools
available today, not to the standard of an internal tool. Concretely:
- **It feels instant.** Every input gets visible feedback within 100 ms; nothing the user did is ever lost or silently
  ignored.
- **It is calm under load.** Streaming output, tool activity and sandbox state changes are easy to follow and never
  jump, flicker or steal the reader's position.
- **It is honest.** The screen always says what the system is doing, what went wrong and what the user can do next.
- **It is effortless to operate.** Keyboard first, obvious next steps, no dead ends, no modal interruptions for
  information that fits in the page.
- **It is consistent.** One visual system, one component per job, the same words for the same things everywhere.
- **It works where teams actually run it.** Any modern browser, light or dark, from a laptop screen to a wide monitor,
  over HTTPS or plain HTTP on a company LAN.
- **It asks for little.** The product decides what it can and asks only what it must (Product Owner, 2026-09-28):
  1. A form shows at most three fields by default; everything else has a good default under "More options".
  2. Screens start from the user's goal, not the data model ("What's this channel for?", not "Name, Purpose, Decision
     rule").
  3. Presets over raw settings: pick a provider, not a protocol, base URL and authentication scheme.
  4. One main action per screen; the rest sits in a "…" menu.
  5. No internal words in anything a user reads (sandbox, egress, admission, route ids, `secrets/…`).
  6. Lists open on what the user needs first (recent, grouped, paged, searchable), never on everything at once.
Every rule below serves one of these. A change that meets the letter of a rule but visibly misses the bar is not done;
the reviewer says so and the change is reworked.

## 1. Character
Quiet, precise, engineering-grade. White paper, black ink, one orange accent used sparingly, generous space, a
restrained type scale, monospace for anything the machine produced. The interface should feel like a well-made
instrument, not a chat toy: no gradients, no glassy panels, no decorative illustration, no emoji in the product UI.

## 2. Brand
- The **TQ mark** (`brand/mark.svg`) sits at the top of the sidebar (§6), 24 px, followed by the text "TensorQuay
  Agent" in the display face. On the sign-in page the wordmark is used at 40 px height.
- The **orange tile** (`brand/logo.svg`) is the favicon and app icon only.
- Orange is the accent, never a surface and never a background for body text: the active indicator bar, the
  streaming caret, an icon or border that must stand out, the mark on the sign-in page. Measured: white text on
  the light accent is 3.6:1, which fails body text, so buttons are ink on paper like the website. Accent may carry
  text only at display size (24 px and above, where 3:1 applies). If more than one element on a screen is orange,
  one of them is wrong.

## 3. Colour tokens
Defined once as CSS custom properties on `:root`, redefined under `[data-theme="dark"]` and under
`@media (prefers-color-scheme: dark)` when no explicit theme is set. Components never use a hex value.

| Token | Light | Dark | Use |
|---|---|---|---|
| `--paper` | `#ffffff` | `#121211` | Page background |
| `--surface` | `#f7f7f7` | `#1a1a19` | Cards, panels, tool cards, inputs |
| `--surface-raised` | `#ffffff` | `#222220` | Dialogs, menus, popovers |
| `--line` | `#e5e5e5` | `#2c2c2a` | Card borders, dividers |
| `--control-border` | `#8a8a8a` | `#767672` | Field, select and text-area borders (3:1 on every surface, WCAG 1.4.11) |
| `--ink` | `#111111` | `#f4f4f2` | Primary text, icons |
| `--muted` | `#626262` | `#a3a39e` | Secondary text, labels, timestamps |
| `--accent` | `#ed542d` | `#ff6a45` | Non-text indicators (3.6:1 and 6.6:1 against paper, above the 3:1 UI minimum), display-size text only, the brand |
| `--ok` | `#186630` | `#5ccb7a` | Success, completed |
| `--warn` | `#875000` | `#f0b35b` | Warnings, waiting for approval, queued |
| `--danger` | `#b4233c` | `#fb7185` | Errors, failed, denied, destructive actions. A crimson red, at least 20° of hue from the orange accent in both themes, so a failure never reads as the brand (WEB-U-089) |
| `--info` | `#2451bd` | `#7aa2ff` | Informational, running, streaming |
| `--selection` | `#dedede` | `#3a3a37` | Text selection |
| `--focus` | `#2563eb` | `#60a5fa` | Focus ring: a calm blue of its own (never the orange accent, which on a field reads as an error), one ring on every control, 2 px (`--focus-width`) at a 2 px offset (`--focus-offset`), one shared rule; 4.8:1 and 6.3:1 or more on every surface (WEB-U-089) |
| `--code-bg` | `#f1f1ef` | `#161615` | Code and tool output blocks |

Status tints for badges and cards use the status colour at 12 % over `--surface` for the background and the status
colour for text and border; never a saturated fill behind text.

Contrast, measured (WCAG relative luminance): `--ink` on paper 18.9:1 and 17.0:1; `--muted` on surface 5.7:1 and
6.9:1; each status colour on its own 12 % tint over surface, light: ok 5.5, warn 5.2, danger 5.5, info 5.5; dark: ok
6.9, warn 7.4, danger 5.7, info 5.8. Every text token on its intended background therefore meets WCAG AA for normal
text (4.5:1), including 13 px labels, and a test recomputes these ratios so a token change cannot silently regress
them (WEB-U-007).

## 4. Typography
Self-hosted, loaded with `font-display: swap`, no external font requests. The app ships Space Grotesk at 400 and 700
and IBM Plex Mono at 400, and every style below is one of those weights: a 500 or 600 would draw as 400 or 700 (17.3).

| Role | Face | Size / line height | Notes |
|---|---|---|---|
| Display (page titles) | Space Grotesk 400 | 30/1.15, letter-spacing −0.04em | Weight 400, never bold for titles |
| Heading (section) | Space Grotesk 400 | 22/1.25 (a page's heading 22/30), −0.03em | |
| Body | Space Grotesk 400 | 16/1.5 (a 24 px line) | Transcript prose, forms, lists |
| Body strong | Space Grotesk 700 | 16/1.5 | Names, emphasis; never for whole sentences |
| Small | Space Grotesk 400 | 14/1.5 | Secondary lines, help text |
| Label | Space Grotesk 400 | 13/1.5, sentence case | Group and section labels, column labels, status pills (§5c: never monospace) |
| Code and tool output | IBM Plex Mono 400 | 14/1.55 | Commands, diffs, file paths, JSON |

The scale is one step bigger since 9 Nov 2026, the Product Owner's ask after comparing the sidebar with a familiar
assistant app's at the same zoom ("seems ours is smaller ... can we adjust all system wide ... bigger"): body 15 to
16, small 13 to 14, the navigation 14 to 15, labels 12 to 13, headings 20 to 22, display 28 to 30, a task's title
16 to 18; controls at least 32 px, list and menu rows 36 px. Icons stay 16 px (20 px in controls and the sidebar's
navigation). Every size is a token, so the whole app follows.

Transcript text wraps at a measure of 72 characters; code blocks scroll horizontally rather than wrap unless the
user toggles wrapping.

## 5. Spacing, radius, elevation
- Spacing scale in px: 4, 8, 12, 16, 24, 32, 48, 64. Nothing else, except the one role value in §5b: 20 px horizontal padding of large buttons.
- Radius: 8 px for controls, cards and dialogs (the standard), 16 px for the composer, 999 px for pills and round icon
  buttons. A control set inside a rounded box at an even inset takes the box's radius less the inset, so their
  corners are concentric: a task row's ⋯ is 6 px, 2 px inside its 8 px row (§7 Conversation list item). No other
  radii.
- Elevation: none by default; dialogs and menus use `--surface-raised` plus a 1 px `--line` border and a soft shadow
  `0 8px 24px rgb(0 0 0 / 0.08)` (dark: `/ 0.4`). Cards are flat with a border.
- Borders: 1 px `--line` everywhere; 2 px `--ink` for the focus ring with 3 px offset.

## 5b. Polish: the details that make it feel finished
A calm, precise interface in the spirit of claude.ai, in our own visual language (Space Grotesk and Plex Mono, paper and
ink, one accent). Every value below comes from the spacing scale in §5; a lint rule rejects anything else.

**Spacing by role**
| Where | Value |
|---|---|
| Page gutter | 24 px (16 px below 768 px) |
| Between page sections | 32 px; between groups inside a section 16 px; between a label and its control 8 px |
| Button padding | medium 8 px × 16 px (height 36), small 4 px × 12 px (height 32), large 12 px × 20 px (height 44) |
| Icon button | 32 px square, icon 16 px, centred; in the composer bar 36 px round, icon 20 px at a 2 px stroke, optically centred (the send arrow drawn 1 px high); 44 px hit area on touch screens |
| Input and select | height 36, padding 8 px × 12 px; textarea padding 12 px × 16 px |
| Card, tool card, approval card | padding 16 px; 12 px between a card's title and body; 8 px between stacked cards |
| Dialog | padding 24 px; 16 px title to body; 24 px body to actions; actions right-aligned, 8 px apart |
| Menu and popover | 4 px outer padding; items height 36, padding 8 px × 12 px; 1 px divider with 4 px above and below |
| Sidebar | items 36 px high (44 px on touch), padding 4 px × 12 px (§5c), 2 px between items; 8 px under the header; 16 px before the task list and between groups; a task's title at the rows' 12 px padding, on the icons' edge (§6); the phone's drawer the same, its rows 12 px from its sides |
| Conversation bar | one line, height 56, aligned to the transcript column, 8 px between pickers; no divider under it (a hairline appears only once the transcript scrolls under it) |
| Transcript | 24 px between turns; 12 px between blocks inside a turn; prompt bubble padding 12 px × 16 px |
| Composer | padding 12 px × 16 px; 16 px from the transcript's last line; the round send button 12 px from the card edge |

**Alignment**
- Everything sits on the 4 px grid; left edges of the conversation bar, transcript and composer share one line.
- Icons are 16 px (20 px in the conversation bar and sidebar), vertically centred with their text, 8 px from it; one icon set, one stroke
  weight (1.5 px).
- Text and icons inside a control are optically centred: the control's height, not its padding, decides its size.
- Truncate with an ellipsis and show the full text on hover or focus; never let text overflow its box.

**Buttons**
- One primary button per view (the most important action); everything else secondary or ghost.
- Labels are verbs in sentence case ("Create administrator", "Allow"); minimum width 72 px for text buttons.
- States, all designed: rest, hover (background 4 % darker in light / lighter in dark), pressed (8 %), focus (the §5
  focus ring, never removed), disabled (40 % opacity, `not-allowed` cursor and a tooltip saying why), loading (a 16 px
  spinner replaces the leading icon or label; the button keeps its width; the action cannot be sent twice).
- Destructive actions use the danger style and name the object ("Delete conversation"); they ask once inline, never a
  bare "Are you sure?".
- An icon-only button's tooltip stays inside the window and the panel it sits in (moved along, or below the button
  when there is no room above), and never shows over the button's own open menu. It is a layer of its own over the
  page, never part of the button's box, and it goes once the pointer moves off the button.

**Type rhythm**
- At most three sizes on one screen. Headings have 24 px above and 8 px below. Body line length 72 characters max.
- Numbers that change (timers, counts) use tabular figures so they do not jitter.

**Motion**
- Hover and press: 120 ms ease-out. Menus, popovers, the approval card and panels: 180 ms, opacity plus a 4 px move.
- Nothing moves layout that is already on screen (no shifting of content above the reader); streaming appends only.
- Skeletons only after 300 ms of waiting; spinners never alone for more than 10 s without a line of text.
- `prefers-reduced-motion` removes all movement and keeps opacity changes only.

**Surfaces and hierarchy**
- Borders before shadows; shadows only on floating layers (§5). One accent colour per view.
- Secondary information in `--muted`; nothing in grey that the user must read to act.
- Empty states: one plain sentence and the one action that fills them; no illustrations in v1.0.

**Checked automatically** (UX suite, WEB-E2E-018 and WEB-U-021): computed paddings, gaps and radii are on the scale;
every interactive element is at least 32 px high (44 px on touch), has hover and focus styles and a pointer cursor (the
active item of a navigation list keeps its selected fill under the pointer, §5c); no text overflows its box at 1024,
1440 and 1920 px; icons share one size per context; a layout-shift budget during
streaming. The technical lead's review adds the visual pass that tools cannot judge.

## 5c. Density: as calm and dense as a familiar chat app
The owner's standard (29 Sep 2026): navigation, menus and tables should feel like the settings and sidebars of
familiar chat apps, not sparse. One rule, set once as tokens in `globals.css`; nothing sets its own row height.

- **Navigation rows** (the Settings menu, the sidebar's tasks): `--row-height-nav` 36 px, padding 4 px 12 px
  (on the §5 scale; the height comes from the minimum), 15/22 text, 2 px between rows; the open one is the active item.
- **Group labels** ("Your account", "Administration", "Today"): 13 px, weight 400, muted, sans, sentence case; 16 px
  above a group, 4 px under the label.
- **The active item** (the Settings menu's item, the open task, the open helper, a chosen tab) is the selected fill,
  `--selected-fill`: 8 % ink over paper, #ececec in light and #242423 in dark, at the body weight, with no accent bar.
  Another row under the pointer takes the lighter 4 % tint (`--hover-tint`); the active item keeps its fill under the
  pointer, never darker, and the open task's title gains no underline (17.3).
- **Table rows** `--row-height-table` 40 px with 8 px vertical cell padding, 4 px around a control in a cell (32 px
  high, so its row stays 40 px); a table with a two-line cell gives every row `--row-height-table-two`, 68 px (§7
  Table); column labels 13 px muted sans.
- **Menu items** (the "…" menus and pickers) are nav rows: 36 px, 15 px text.
- **Touch:** on `pointer: coarse` screens nav and table rows grow to `--row-height-touch`, 44 px, as tap targets; a
  control in a table cell grows with them, to the row's 44 px.
- **Page headings** such as "Settings" are 22/30; the content starts 24 px under them.
- **Monospace only for values that are code or ids** (a host, a command, a model id), never for labels.
- Check: a menu of 13 items fits without scrolling at 900 px high; compare screenshots at 1280 and 390.

## 6. Layout
- **App shell (layouts A + C: full sidebar by default, collapsible to a rail; product decision 24 Sep 2026): no global header.** A left sidebar
  and the content column, nothing above them. The sidebar has two states, switched by an icon button at its top
  (accessible name "Expand sidebar" / "Collapse sidebar", key `[`), remembered per browser:
  - **Expanded (default), 260 px:** the mark and product name (a 40 px row) with the toggle at its right; 8 px under
    it, the top block, "New task", "Workspaces" and "Channels", in that order and one row style (navigation rows 2 px
    apart, §5c); one group gap (16 px) under it, the task list by date; the account row alone at the bottom (§7
    Account menu). One left edge for the mark, every item's icon, the group labels and the task titles (no icon column on
    a task row; the owner, 9 Nov 2026); the toggle sits on the rows' ⋯ column. An item whose label shows
    has no native tooltip. Conversation 720 px.
  - **Rail, 56 px:** the global actions only, each with its accessible name and the app's own tooltip beside it on
    hover or keyboard focus (never a native title; the toggle keeps its title with its key): the mark, the toggle, "New
    task", Search (when the list offers it), Workspaces and Channels, and the account avatar at the bottom; of the task
    list, the open task alone, its icon carrying its run mark; no other tasks, status marks, chevrons, counts or "Show
    more". The conversation keeps its one width (720 px). The rail is also the default between
    768 and 1024 px.
  Below 768 px the sidebar is a drawer opened from a menu button at the left of the conversation bar. The content
  column starts with the conversation bar. The shell fills the viewport and never scrolls as a page: the sidebar stays
  in place, and each content area scrolls on its own. Budget: the first transcript line starts at most 64 px below the
  top of the window (it was about 175 px with the header). Reference mockups: layouts A (expanded) and C (rail),
  24 Sep 2026.
- **Conversation page:** a single centred column of maximum 720 px for the transcript, which is the page's only
  scroll container, a composer pinned below it, and an optional right panel of 360 px for files and diff, sandbox
  status and context. Where the task's own layout is at least 900 px wide (from 1024 px with the rail, from
  1208 px with the full sidebar) the right panel docks beside the transcript, which narrows; with less room it takes the
  whole content column, from under the bar (its second line too, on a phone) to above the composer, and never covers
  part of the conversation (WEB-U-170, WEB-U-171). The layout's width decides, not the window's. It never opens by itself (candidate12
  F6): closed until the person opens it, their choice remembered on wide screens; Open, Open changes and a file link
  open it on what they name.
- **Admin and settings:** an 800 px content column (760 px before the type step of 9 Nov 2026, §4) with section
  headings on the left on wide screens.
- Page gutters: 24 px, 16 px below 768 px. No horizontal page scroll at any width.
- Density: one density. No compact mode in v1.0.

## 7. Components
One implementation per component, in `web/src/ui/`, tokens only, keyboard operable, labelled.

| Component | Variants and rules |
|---|---|
| Button | `primary` (`--ink` background, `--paper` text; dark theme inverts, both above 17:1), `secondary` (surface with border), `ghost`, `danger` (`--danger` text and border on surface, filled `--danger` with paper text only in confirmation dialogs at 16 px bold, 5.5:1 light). 36 px height (in a table cell 32 px, 44 on touch, §7 Table), 12 px horizontal padding, label in body strong. One primary per view. Loading state replaces the label with a spinner and keeps the width. A list row's Edit is a `ghost` button, never an underlined link, the same whether it opens the item's page or a dialog (17.3, WEB-E2E-108) |
| Input, textarea, select | 36 px, `--surface` background, `--line` border, `--ink` focus ring. Errors show `--danger` border plus a message below in Small. A form's select is the standard picker (the shared `Select`, WEB-U-134): its label above a field-looking button with the choice and a chevron, opening the standard menu; never a native `<select>`. The route form's Shared key stays a typed field with suggestions, since it must refuse a pasted key (SEC-033), and looks like that picker: the browser's own list indicator hidden, the standard 16 px chevron inside its right edge |
| Checkbox and radio | One shared check row (`CheckRow`, src/ui): the native checkbox or radio, kept for its keyboard, its label and its form, fills a 32 px target with a pointer, transparent, over Lucide's 16 px square (a circle for a radio), the checked square (the dot) when on, in `--control-border`, `--ink` when on; its words after it make the whole row its label; disabled at 40 %; the focus ring around the target. Never a bare native checkbox (WEB-U-434) |
| New conversation | No dialog: `/` and "New conversation" open an empty conversation page with the composer focused and a one-line prompt ("What should the agent do?"); the model and environment pickers in the conversation bar above the transcript. The model starts on the first route whose check didn't fail; routes that would read the same add what tells them apart (the model's name, else where it runs, else a number), never a route's id; a route whose last check failed says "Its last check didn't pass" as quiet text on its menu row only. Under the composer, suggestions as chips of one style (the `--surface` fill, a 1 px `--line` border, round ends, 13 px text), in a row from the left that wraps, 8 px apart; a click puts the suggestion in the box. While no sites are allowed, an inline note under the composer, from its left edge: a 16 px info icon and one `--muted` line; an administrator's says "Tasks can't reach the internet: no sites are allowed yet." with Allow sites as a small ghost button, and a member's, with no action, "Tasks can't reach the internet: no sites are allowed yet. Ask an administrator to allow sites."; at a phone's width the line wraps balanced, never leaving a word alone. Created on first send, titled from the first line of the message, renamable inline in the conversation bar |
| Conversation bar | The top bar of the conversation column (the UI standard's header), one line (on a phone, a task's model and environment take a quiet second line under it, below): the title, 18 px weight 700, renamed in place from a click on it or the ⋯'s Rename ("New task" before the first message), with one quiet line under it for the agent profile it came from and the workspace it is in when that is one made or shared ("Agent: Analyst · In Team handbook"); the run status in words right after it in Small type ("Done · 5 s · Asleep, wakes when you send"; the state word in its colour, the rest muted); then, on the right, the **model picker** (display name, for example "DeepSeek Flash ▾"; a menu of the available models, each with its muted second line) and the **environment picker** ("Workstation ▾", ARCHITECTURE §4.7) as small text buttons (the name and a chevron, no box; muted while a run or the task's profile fixes them, with the reason), each opening below, aligned to its end, and applying at once; a quiet Files icon (its word in a tooltip) that opens and closes the files panel; and one ⋯ (Rename, Keep environment on, Delete task). Keep environment on is the task owner's keep-alive pin (FR-33): a checked toggle, with one muted line under it on what it costs ("Counts as a running task"). On New task the same two pickers sit in its top bar and set what the task will use; on a task the model picker changes the route for the next message and the environment picker asks to switch, just under the header, while no run is active. On a phone (the header under 520 px wide), a task's header keeps the title, the status, the Files icon and the ⋯ on its first line, and its model and environment pickers take a quiet second line under the title, with their names and chevrons, one tap each (WEB-U-154); New task's top bar, whose title is short, keeps them beside it. Elsewhere, where the pickers' names don't fit beside the title's least room (8rem) and the status's (4rem), which the bar measures (beside the files panel only with long names), each picker shows its icon (Sparkles, Box) alone, never a cut name: the chosen name is its accessible name, its tooltip on hover or keyboard focus, and its menu's title. The status is one short word at every width, "Working…", "Done", "Failed", "Not sent", and from v1.1 "Waiting for you" (§13b) (WEB-U-155): the reason and the elapsed time are in its tooltip, still read by assistive technology, and the transcript's (the turn's outcome, the live line); so the title keeps the width, and truncates only when the bar is narrower still; the pickers, the Files icon and the ⋯ keep their size and stay inside the bar |
| Composer | One card on `--paper` (never a grey fill), 1 px `--line` border, 16 px radius, the soft dialog shadow, and a 2 px `--ink` focus ring when the text has focus (the UI standard's composer). Top to bottom: New task's chips for what "+" added (a workspace, an agent profile), each a small quiet picker with its ×, which goes back to the default; the files attached, as chips (an image's chip shows the image itself as a 24 px thumbnail in place of the file icon); the auto-growing text (2 lines when empty, growing to 8, after which it scrolls; Enter sends, Shift+Enter newline, placeholder "What should the agent do?" on a new conversation and "Reply…" after); and one row of two **icon buttons that share one implementation** (`IconAction`, round: 36 px, a 20 px Lucide icon at a 2 px stroke, optically centred, a tooltip on hover and on keyboard focus, the §5b hover tint; WEB-U-428): **+** (on a task it opens the file chooser, its tip "Attach files" with the limits as its description; on New task it opens a menu of Attach files, Workspace… and Agent profile…, its tip saying so, and a chosen item's picker opens after it, never on top of it; a chip's status, such as a workspace in use, sits right under the chip), then, at the row's end, the one that carries the state: **Send** (arrow-up icon; filled `--ink` with a `--paper` icon only while there is text to send, its tip "Send · Enter"; otherwise an outline disc at 35 % ink with nothing filled and no hover tint, so it reads as waiting, never as faded or faulty (§8), its tip "Type a message to send"; an attachment alone leaves it so), **Sending** (spinner in place of the icon, not clickable), **Stop** (square icon on a `--surface` disc with a `--line` border) while a run is active, which stops it at once (only a double-click's second click is ignored). The button's accessible name follows its state. The model and environment are not here: they are the header's (above). The keyboard hint ("Enter to send · Shift+Enter for a new line") is the placeholder's second line on New task while the person has no tasks yet, so it goes with their first message, which makes their first task; it is never a line of its own, and never where the pointer is a finger (a touch keyboard has no Shift+Enter; WEB-U-154, WEB-U-431). A disabled composer says why inside the card ("The environment is starting…"). **Paste and drop** (WEB-U-427, WEB-U-020): a screenshot or a file on the clipboard, pasted anywhere in the card, attaches at once, the same way as a file chosen with + (the same per-file and per-message limits, the same refusals); an image with no name of its own is "pasted-image-N.png" (N counts the card's pasted images; a JPEG gets .jpg), a name the gateway keeps, so the chip reads the same before and after the upload; a pasted file keeps its name; pasted text is text. Files dropped anywhere on the card attach the same way; while they hover, the card shows a dashed `--ink` outline and says "Drop files to attach" above the chips; a drag that carries no files changes nothing. Where files can't be attached (sending, a disabled composer), a paste or a drop attaches nothing; on New task they are held as chosen files are, until the task exists |
| Status pill | Label face, 22 px, tinted per status: running (`info`), waiting for you (`warn`, v1.1 helpers, §13b), waiting for approval (`warn`), queued (`warn`), sleeping and idle (`muted`), completed (`ok`), failed and denied (`danger`). In Settings, green is only for a check that passed ("Verified", "Ready", "Available"); a plain state or choice ("Default", "On", "Active", "In new tasks", an accepted invitation, a ready-made list that is on) is muted, its word saying it (17.3) |
| Message | User messages right-aligned in `--surface` with a 6 px radius; assistant messages full width on paper with no bubble; reasoning collapsed by default under a "Reasoning" disclosure in Label face |
| Tool card | `--surface` card with a header row (tool name in mono, status pill, duration), the command or arguments in a code block, streamed output in a code block capped at 20 lines with "Show all", the exit code, and a diff view for file edits with added and removed lines tinted `ok` and `danger` at 12 % |
| Approval card | Inline in the transcript, `--warn` tinted, with the bounded summary in plain words (a command in a code span), a quiet countdown from `expires_at`, and two buttons: Allow (primary) and Deny (danger ghost); keyboard A and D; focus moves to the card. "Allow for this conversation" (secondary, key S) joins when session grants land (task 041). A risk badge only when the request carries a risk level. After a decision the card shows "Allowed" or "Denied" quietly |
| Conversation list item | Title (never a slug), a status shape with its word for assistive technology (working, failed; none when quiet), a muted time or date only when another task in its group has the same title; the workspace name on hover; Rename and Delete in a menu from its ⋯, which is part of the row and never a chip over it: on hover, on the row's own keyboard focus (never for a focus a click left behind) and while its menu is open, it shows inside the row's backdrop at the row's end, square, 2 px from the row's top, bottom and end (`--row-inset`), its corners concentric with the row's (6 px), with no fill of its own; its hover and press are the quiet tints inside that backdrop, and keyboard focus keeps its ring; while it shows, the marks at the row's end give way and a title that reaches it fades out before its edge (a mask), never cut by it; without hover, titles keep the full width; on a touch screen it always shows, and the title and the marks end before it; the rail has none; a row's title never underlines on hover, since the row's backdrop shows it; grouped Today, Yesterday, Previous 7 days and Older, eight a group until "Show more", a small ghost button (§5b: 32 px, 12 px each side) at its own width under the group, its label starting where the titles do, so it reads as an action and never as one more task (17.3); the open task is §5c's active item: the selected fill at the body weight, kept under the pointer with no underline, and no accent bar |
| Workspace card | Name, owner, size, last used, lease state ("in use by …"), shared badge; a menu with New task here, Fork, Share, Rename and Delete (danger, confirm) by access. A table row on screens from 768 px: the row itself opens (click or Enter; rows are focusable) and Open is the menu's first item; the name takes the spare width, sharing is a small people icon and count after it, and the owner shows only when it isn't you. A card below 768 px, with an Open button |
| Channel post | A light row, not a card: the author's name ("Agent" beside a task's agent), a role badge only when it isn't a plain member, then the body; consecutive posts by one author within five minutes share one header line, as in chat apps; seq, time and Reply are one quiet line at the end of the author's line, shown on hover or focus (always on touch screens); replies indented under their parent; a proposal shows one tally bar with the counts in words; decisions are `--info` tinted; tasks show claim state |
| Auth pages | Setup and sign-in: a centred 400 px column on `--paper` with no card; the wordmark, one heading, one line of help, bordered inputs, the primary button full width; password rules listed under the field and ticked as they are met; a show/hide toggle. Creating the first administrator signs them in. `/login` redirects to `/setup` while no account exists; a failed sign-in says "Email or password is incorrect" |
| Sidebar | A "New conversation" button, then the conversation list (title, relative time, the open task in §5c's selected fill), and at the bottom the **account row** (avatar with initials, the email or name, truncated). Only built features appear; administration appears for administrators only. No disabled placeholders |
| Account menu | Opened from the account row at the bottom of the sidebar (never the top bar), opening upward (WEB-U-463): the email and role (muted); Settings, Usage (administrators), Language (the current language's own name, a submenu of all available languages each written in its own script), Help and Send feedback (the project's pages, a new tab, said to screen readers); a hairline; API tokens, About (a submenu: what's new, the source code, the licence); a hairline; Sign out. Each item has its 16 px line icon. Theme is in Settings › Account, not here. Keyboard operable; Escape and outside click close it; focus returns to the row. On a collapsed sidebar the row shows the avatar only |
| Dialog | Centred, 480 px, title in heading face, actions right-aligned, Escape closes, focus trapped |
| In-place edit | For one short text (a task title, a workspace name), as on Claude.ai: at rest plain text; on hover only a subtle tint behind it, no box and no pencil; then the text itself becomes the field, at its own font, size and place, all of it selected, with no visible label, no buttons and one thin underline; a focus ring only when the keyboard opened it (the ⋯'s Rename by keys); nothing around it moves. Enter, a click elsewhere or moving focus away saves; Escape cancels and returns focus to what opened it; an empty or unchanged text keeps the old one; a save that fails keeps the text and says why. Built on `InlineEdit` and `useDismiss` (src/ui), never a one-off |
| Action menu | Actions on one thing (a task row, a Settings row, a member, a workspace, a channel), from its ⋯ or a right-click on it: a 16 px line icon before every item, the word, and under it why an action can't run when it can't (the item stays reachable, dimmed); a destructive item in the danger colour; a setting turned on and off here is a checkbox item (`menuitemcheckbox`), its check at the row's end while on, with what it means or costs in one muted line under the word, at most 36 characters, so the menu keeps the standard 200 to 240 px (one type step up since 9 Nov 2026, §4). It closes after a choice, on Escape or a click elsewhere, and focus returns to the ⋯ before the action runs. Built on `ActionMenu` (src/ui), never a one-off |
| Toast | Bottom-right, 5 s, one line, with an optional action; errors persist until dismissed |
| Table | For admin lists: 40 px rows (`--row-height-table`, 44 px on touch), 8 px vertical cell padding, 13 px sans column labels, zebra off, hover `--surface`; a control in a cell (Edit, the ⋯) is 32 px high (44 px on touch) with 4 px above and below, so a one-line row stays 40 px; a cell is its value and at most one muted line: a longer line is cut with "…" and keeps its words whole in its tooltip, and the item's own page says them in full (where no page says a note, such as an engine's budget use, the tooltip is its home); a table with any two-line cell gives every row the same two-line height, 68 px (`--row-height-table-two`), and a one-line table stays at 40 px; a column whose cells carry a muted line keeps at least 16rem (`--table-note-width`), so its notes read as words, a note that leads with a link keeps the link's width (a link is never cut), each other column takes its content's width, and the name column takes the rest, at least 12ch; on Limits the value column takes the rest instead, since the notes are the value's, and the names keep their own width; a source or other plain words that belong to the item go on its muted line, never in a column that takes the name's room; a value that may be long (an address, a command, a last use, an email) keeps its own width up to 16rem and is cut past it, whole in its tooltip; in a dialog's table each column keeps its content's width, its short notes whole, and the column of who did it takes the rest; numbers sit on the right in tabular figures (17.3); a column appears only when at least one row fills it (Owner on Workspaces only with someone else's workspace listed); a name is never cut while the row has room; where the table's own column is under 600 px (a phone, or beside a menu) its rows are cards instead of a table to scroll sideways: the row's name first with its ⋯ beside it, then each other cell under its column's name, the header kept for screen readers (WEB-U-190); a row's actions are under its ⋯, a destructive one as a danger item that asks first, never a bare button (WEB-U-191) |
| Segmented control | For a page's filters: one `--surface` tray, the chosen segment a `--paper` fill at the body weight, the others muted; 32 px segments; never a row of bold tabs. At phone width (under 768 px) the same choices are the standard picker, labelled as the control is ("Show: All"), never a row of labels to scroll sideways (17.3) |
| Tabs | Views of one place: a task's Files panel (Files, Changes, Context, Screen), a channel's side (To-dos, Members) and Keys (Mine, Shared). One shared tab list (`TabList`, src/ui) drawn by one style (`.tab-list`, `.tab`): the chosen tab is a soft filled pill, §5c's selected fill with its label in ink; the others are plain muted labels that take the hover tint under the pointer, where the chosen tab keeps its fill. One weight (400) for every tab, never an underline or a bolder chosen tab; 15/22 text, the label centred; 32 px high (44 px on touch screens), 12 px inside, 8 px corners, 4 px apart. A strip wider than its place (a long translation on a phone) scrolls sideways inside itself, never the page, and keeps room for the focus ring. The keyboard is WAI-ARIA tabs': the chosen tab is the list's one Tab stop; the arrow keys choose the next or previous tab and wrap, Home and End the first and last, the focus moving along; each tab names the panel it controls. Tabs that open other pages (Keys' Mine and Shared) stay links in a labelled navigation, the current one marked, drawn the same. A page's filters are the segmented control, never tabs (9 Nov 2026) |
| Empty state | A 20 px icon that names the thing, in `--muted` (the sidebar's icon for that page; never the logo), one sentence, one primary action |
| List tools | A search, filter or sort appears only over a list long enough to need it: 8 items or more, or more to load, or while one of them is in use so it can be undone. Fewer items show without them; a short picker is plain buttons. An empty list in the sidebar is one quiet line ("Your tasks will appear here."), no mark, since New task sits just above |
| Skeleton | For loading lists and history: `--surface` blocks with a subtle shimmer; never spinners for page loads |

Icons: Lucide as a dependency, 16 px in text and 20 px in controls, stroke 1.5 (2 in the composer bar's round buttons, §7 Composer), `--ink` or `--muted`. No custom icon
drawings except the brand mark.

## 8. States and motion
- Every data view designs loading, empty, error and partial states before the happy path.
- Streaming text appends without layout shift; a thin `--info` caret blinks at the end while streaming, right after
  the last word (in the last paragraph, heading or list item). A step's narration is done once the step's next call
  starts, and its caret ends there.
- The transcript follows new content while the reader is at the bottom; scrolling up pauses it and shows a
  "Jump to latest" button, with the count of new items only when there are any ("Jump to latest · 3 new", never
  "(0 new)"); sending a message resumes following (ARCHITECTURE §11).
- Counts in copy agree in number: "1 match", "2 matches", "1 file", "1 command".
- Assistant text renders as Markdown (headings, lists, emphasis, inline code, fenced code with a copy button, tables,
  links) in the transcript's type scale; raw HTML from the model is shown as text.
- Motion: 150 ms ease-out for hover and focus, 200 ms for panels and dialogs, no bounce, no parallax. Everything
  respects `prefers-reduced-motion` by rendering end states.
- Live activity: while a run works, the open activity group's last line names the step it is on, from the call that
  runs now (its name and its first argument, never the model's words): "Reading README.md…", "Writing report.md…",
  "Running the tests…" for any test runner, "Looking at the screen…", another command by its first words, and
  "Thinking…" while no call runs. A file is named by its name; its whole path is on the call's own line. The step's
  time follows the words ("Running the tests… · 12 s"), counted from the call's start. A soft light crosses the words,
  never the time, every 2 s while the step runs; reduced motion and forced colours show the words still, in the line's
  colour. Each call is its own line from the moment it starts, never before, with a small mark (a loader) turning
  after its words (its name still says "Running"); when it ends, it settles into its done line with how long it took
  after any mark ("Created hello.py (+2 −0) 120 ms"). A running command's open card shows its latest 20 lines as they
  come, and its first 20 once it has ended. Reduced motion holds the mark still. While the model thinks, its thinking
  streams in an open panel under the line, each part fading in as it arrives and the panel following its newest line
  (the line folds it); once the model starts writing, the thinking folds in its place to "Thought for 12 s", and no live
  line speaks over the words streaming with their caret.
- One run mark: while a task's run is under way, a calm pulsing `--info` dot (the breath of a workspace held by a run)
  sits before its status beside the title in the bar, the same dot is its sidebar item's mark once the list has the
  run (a first message still waiting for a slot is not a run yet), and its tab title starts with "● "
  ("● Puzzle 17.2 · TensorQuay Agent"). "Starting…" and "Waiting for a free slot · 2 ahead" keep their words with it;
  done, stopped or failed, there is none. Reduced motion holds it still.
- Sandbox transitions are calm, never a modal, and said in the run status beside the task's title: an environment
  starting or waking reads "Starting…", one waiting reads "Waiting for a free slot · 2 ahead". An environment that
  sleeps after idling is not news: the header says nothing of it, and the next message starts it again ("Starting…").

## 9. Accessibility
- Keyboard operable end to end; visible focus ring; skip link to the transcript; focus returns to the trigger after
  a dialog closes.
- Every icon-only control has an accessible name; every form field has a label; errors are announced.
- Colour is never the only carrier of meaning: status pills carry text, diffs carry `+` and `−`.
- Automated checks on every page in CI (WEB-U-007); manual keyboard walk-through per phase demo.

## 10. Copy
- Plain, short, present tense. Sentence case everywhere, including buttons ("Allow once", not "ALLOW ONCE").
- Say what happened and what the user can do: "The sandbox is waking. Your message will send in a moment."
- No exclamation marks, no jokes, no anthropomorphising the agent beyond "the agent".
- Times are relative in lists ("4 min ago") and absolute on hover. Numbers use narrow no-break spaces (U+202F) for thousands, so a number never breaks across lines. Token counts read
  in their language's short form (2.8M, 282.5万, 2,8 M; one decimal at most), every count on a page the same way; a
  standalone figure keeps its exact number on hover and for screen readers (WEB-U-461).
- Run status in words the user would use: "Working…" (with elapsed time), "Done · 22 s", "Stopped", "Failed: <reason>".
  The execution environment is mentioned only when it affects the user: "Starting the environment…", "The environment
  is asleep; it wakes when you send." Internal names (route ids, "sandbox", "idle") never appear in the product UI.
- **The unit of work is a "task" in the product UI** (product decision, 24 Sep 2026): "New task", "Tasks" in the
  sidebar, "Delete task", "Rename task", "Allow for this task", "Untitled task". The API, protocol and code keep
  "conversation"; only user-visible copy changes. This document's own component names ("conversation bar",
  "conversation page") are internal and stay.
- The browser tab title is "<task title> · TensorQuay Agent", or "New task · TensorQuay Agent".
- **Helpers (v1.1):** a task that leads others is the "lead task", the others are its "helpers", and together they are
  a "team" ("Stop team", "3 helpers"). Never "member", "agent team", "role", "agent_team" or a helper's internal name
  in the product UI. A helper is named by its job in words ("Researcher"), as the lead gave it.

## 11. Implementation rules
- `web/src/app/globals.css` defines the tokens; Tailwind's theme maps to them; components use Tailwind classes that
  resolve to tokens or the CSS variables directly. A lint rule rejects hex, rgb and hsl literals outside
  `globals.css`.
- `brand/` is copied into `web/public/brand/` by the build; the app never holds a second copy of an asset.
- A `/design` page renders every token and every component in every state, in light and dark. It is the living
  reference and the target of the visual regression test (Playwright screenshots compared per component). It is a
  developer tool, not a product page: it is never linked from the navigation, and a production build does not serve it
  (404); development and test builds do.
- Component files stay under 400 lines; one exported component per file; variants through props, not new files.
- Dark mode follows the system by default and can be pinned in settings; the choice is stored per user.

## 11b. Languages (internationalisation)
- Every visible string, tab title, live-region announcement and error copy comes from a translation catalogue; no
  hard-coded user text in components (a lint check enforces it).
- Catalogues are plain files, one per language (`web/src/i18n/locales/<code>.json`, ICU message format for plurals and
  variables); the app discovers languages from the files; a missing string falls back to English, never to a key.
- First languages: English, Simplified Chinese, Japanese, Korean, Vietnamese, Thai, Malay, Tamil, French, Spanish.
  The browser language picks the default; the Account menu changes it and the choice is remembered per user.
- Numbers, dates, relative times and plurals use `Intl` for the chosen language; layout tolerates 40% longer strings.
- **Simplified Chinese spacing:** a half-width space between Chinese and Latin letters or Arabic numerals, on both
  sides, including before a unit or measure word: "Ollama 为这个模型提供 4 096 个令牌", "{seconds} 秒后重试",
  "已 {minutes} 分钟". No space before % or ° ("{percent}%"), next to full-width punctuation ("（Mac 或 Windows）"), or
  around a placeholder that stands for words ("与{against}相比"); a number placeholder counts as a number. `Intl`
  attaches 秒 and 毫秒, so the duration formatter adds the space (WEB-U-305).
- Fonts: a fallback stack covers every shipped script (Thai, Tamil, CJK, Hangul) with no tofu boxes.
- Model and agent output is never translated; only the product chrome is.
- Adding a language is one pull request with one file (CONTRIBUTING, "Add a language"); CI reports missing strings per
  language without failing the build.

## 12. Interaction and responsiveness
- Feedback budgets: visible response to any click, key or submit within 100 ms (pressed state, spinner in place of a
  label, optimistic row); a page or panel shows its skeleton within 300 ms; anything longer than 1 s shows progress in
  words ("Starting the sandbox…").
- Sending a message is optimistic: the user message appears at once in a pending state and is confirmed or marked
  failed with a retry, keeping its text; the composer clears only when the gateway accepted the message.
- Keyboard: Enter sends and Shift+Enter adds a line; Escape stops the running run after a confirmation that is itself
  keyboard operable; `/` focuses the composer; ↑ in an empty composer recalls the last message for editing. Shortcuts
  are listed in a help dialog (`?`) and never override the browser's own.
- No work is lost: a draft in the composer survives a reload and a navigation away and back; a failed send keeps its
  text; closing a dialog with changes asks first.

## 13. The agent conversation
- The reader always knows the state of the run from the header pill alone (queued, starting, running, waiting,
  completed, failed, cancelled) and of the sandbox (sleeping, waking, ready).
- Streaming text renders as Markdown as it arrives (§8), with the caret at the end; the transcript follows new content
  while the reader is at the bottom and never moves while they read earlier content (§8, ARCHITECTURE §11).
- The transcript tells a story, not a log (ARCHITECTURE §11.2): each turn is the user's prompt, an activity group of
  one-line tool summaries ("Created `hello.py`", "Ran `python3 hello.py`"), and the answer as the visual anchor.
  While the run is active the group is open with a live last line (folded by the person, the live line is its heading
  instead: the step, its shimmer and time, the button's name unchanged, WEB-U-462); when it completes the group folds
  into one line ("Worked for 38 s · created 2 files, ran 3 commands"); a failure keeps it open at the failing line. Each line opens
  into the full tool card; long output is capped with "Show all"; copy is available on commands, code and output.
- Reasoning is available but secondary and visually quieter than the answer: while the model thinks, it streams open
  under the live line (one click folds it); once the model writes or acts, each step's thinking folds in its place to
  "Thought for 12 s", one click to open (§8).
- Errors from the model, a tool or the sandbox appear in the transcript at the place they happened, in plain words,
  with the action that fixes them (retry, start a new run, check the model route), never only in a toast.
- Switching the environment (ARCHITECTURE §4.7) happens from the environment picker in the conversation bar while no run is active; one
  inline line states the consequence ("Files and history stay; anything still running stops"), and the transcript records
  "Switched to <template>". A task on a Desktop environment also has the Screen panel, which shows the agent's
  desktop live (§13d).
- Long transcripts stay fast (WEB-E-009); history loads without a spinner wall; scrolling is smooth.

## 13b. Helpers (v1.1, ADR-0034)
A task can lead helpers: it starts them, gives each a brief, reads their reports and decides. **A person starts a team
by asking in words**, in any task ("get two helpers to check each region"). There is no control for it: no button, menu
item or form, and none is added later. This section is how a team looks once it exists. It follows the UI standard's
controls table and adds no control of its own. Nothing in it moves or opens on its own (§0, §5b).

| What | Where | Control | Behaviour and words |
|---|---|---|---|
| **The task list grouping** | Sidebar | Conversation list item, as a tree disclosure | One 36 px row per task, as now (§5c), never a second line. A lead task's row starts with a chevron spanning the row's start padding and its icon (28 px, as tall as the row), its icon on the sidebar's one left edge (§6), and its title 4 px after it, so the row reads as one row; it ends with its helper count in `--muted` ("3"); when a helper is stuck or failed, the count's place says "1 stuck" or "1 failed" in `--danger`, while the team runs only (an ended team's row shows its count in `--muted`, since nothing in it waits on anyone). The chevron opens and closes the helpers' rows, which start where the lead's title starts, each one line with its name ("Researcher") and its own status shape. Closed by default, and only the person opens it. Helpers never appear as rows of their own at the top level, and follow their lead's date group. Search finds a helper and shows it under its lead |
| **The team strip** | Lead task's page, one line under the conversation bar | Page content; the chips open pages, "All helpers" opens a Side sheet | One quiet line: a chip per helper (state dot and name), then "Budget 20 % used · 1 h 12 min left" with a thin meter; the exact figures are in the tooltip. Over 4 helpers, "+2" ends the row. At 390 px the line is "3 helpers · Budget 20 % used ▸", which opens the Side sheet. A chip opens that helper's page. Once the team no longer runs, the budget and time give way to how it ended, with no meter: "Finished · 20 % of the budget used", "Out of time · …", "Stopped · …", "Used its whole budget", or "Stopping…" while it stops; the chips stay, to open each helper's page. The word "tokens" never appears in the product (PRD §12 item 11) |
| **All helpers** | Side sheet from the strip | Side sheet; each row has Open and an Action menu | One row per helper, saying what it is doing: name; state in words; while working, its current step in words and for how long ("Running a command for 3 minutes", "Thinking"), never a tool's name; its last message to the lead, with its kind and when ("Reported · 12 min ago") and its text in one muted line; its share of the team's work so far ("25 % of the team's work so far"). Then **Open**, and ⋯ with Stop helper while the gateway says it can be stopped (`can_stop`): a done helper, and one whose run failed, still hold a place until stopped; none for a stopped helper, one that never started, or once the team has ended. No forms in the sheet |
| **Helper states** | Strip chips, sheet rows, the helper's own status | Status pill words | Seven states: "Working…", "Waiting for the lead" (also a helper that stopped without reporting), "Waiting for a slot (2 ahead)", "Stuck", "Done", "Stopped", "Failed". Stuck and Failed use `--danger`; Waiting uses `--muted`; Working uses `--info` |
| **Team messages** | Lead task's transcript, in order with its turns | Quiet lines, like the activity group's (§13) | One muted line each, 13 px, with a small arrow icon: "To Researcher: Find the Q3 revenue…", "Asked all helpers: …", "Researcher answered: …", "Researcher reported: … · 2 files", "You told Reviewer: …". A click opens the full text in place. A decision is the same line tinted `--info`. There is no Veto button: a person who disagrees writes to the lead task ("No, use format Y"), which the lead reads; one way to say it |
| **A helper's page** | The helper's task page | Page; the header's quiet line | Under the title, a quiet line "Helper of Report on Q3 · Researcher", where the lead task's title is a link. It is one line at every width: the lead's title gives way with "…" (whole in its tooltip), and the helper's name stays. The helper's own copy of the files, named like the helper, isn't named again ("In …"); a workspace it shares is. The transcript streams live, like any task's |
| **The brief** | First item of a helper's transcript | Card in the transcript | A `--surface` card titled "Brief from the lead": Objective in full, then Output, Sources and Boundaries when the lead gave them. Over 6 lines it folds, with "Show the whole brief" |
| **The composer notice** | A helper's page, under the composer card | Quiet line, like the composer hint | "The lead task sees what you send here." It shows every time while the team runs, because it isn't a hint about keys but a fact about who reads; once the team has ended, the lead is no longer told, and the line goes. The placeholder stays "Reply…" |
| **Waiting for you** | Lead task's status, list marker, transcript | Status pill, list status shape, and a card in the transcript | The status word "Waiting for you" (`--warn`). In the task list, a filled amber dot with the words for assistive technology; the row stays where it is. In the transcript, a `--warn` tinted card with the lead's question and "Answer in the message box below"; sending answers it, and the card then says "Answered" quietly. The lead may keep working meanwhile: the status stays the one word "Waiting for you" while a question is unanswered (it wins over "Working…", which goes in its tooltip, WEB-U-155) |
| **Stop team** | Lead task's ⋯ menu | Action menu item, then a Dialog | "Stop team" (a danger item, shown while the task leads helpers). The Dialog says "Stop the team? 3 helpers stop now. Their tasks and files stay." with Stop team (danger) and Cancel. The composer's Stop still stops only the lead's own run, and says so while helpers work (its tooltip and description: "Stops the lead's own step; its helpers keep working and can wake it. To stop them all, use Stop team in ⋯."); once it has stopped the lead, a quiet line under the strip says "Helpers are still working." with a Stop team button, the same action, for a touch screen. A helper's ⋯ has "Stop helper", which asks the same way: "{name} stops now. Its task and files stay. The lead is told not to redo its part." (a person's Stop means not this part; the lead isn't to hand it to a new helper unless asked) |

**States.** Every part above designs these before the happy path (§8):

| Part | Loading | Empty | Error | Partial |
|---|---|---|---|---|
| Task list grouping | The lead's row as usual; the helper rows as skeleton lines when opened | No "helpers" line: a task without helpers looks like any task | "Couldn't load the helpers. Try again" as one quiet line under the lead's row, with Try again | A helper deleted since: the count drops, and the lead's transcript has a line "A helper was deleted" |
| Team strip and sheet | Skeleton chips | Not shown | "Couldn't load the team. Try again" in the strip's place | A helper whose state can't be read shows "State unknown", in `--muted`, never a blank |
| Team messages | Skeleton lines with the transcript's history | None until the first helper starts | The transcript's own error line, with Try again | A message too long for a line ends in "…", and opens in full |
| The brief | Skeleton card | Not possible: every helper starts with a brief | "Couldn't load the brief. Try again" in the card | — |
| Waiting for you | — | — | An answer that fails to send stays in the box and says why, as any message does | — |

## 13c. Goals across teams (later, ADR-0038)
A goal is one person's piece of work split across several teams, each with a lead task, and a coordinator that talks
to the leads. It reuses §13b's parts; this section adds only what is new. Words: "goal", "part", "team", "Coordinator"
(kept, ADR-0038), "lead task" and "helper".

| What | Where | Control | Behaviour and words |
|---|---|---|---|
| **Setting up a goal** | Its own page | Page (the controls table) | The goal in words, then its parts: each a name ("Finance: the numbers") and an agent profile, chosen from the person's own and the shared ones; the coordinator; the budget and time within the administrator's limits. Save and Cancel pinned at the bottom. When the coordinator proposes the parts, the same page opens filled in, for the person to confirm, change or decline |
| **The goal page header** | Goal page | The conversation bar's layout | The goal's title (renamed in place), its status in one word, and one ⋯ with Pause goal (Resume while paused, and the status says "Paused") and Stop goal (danger, then a Dialog: "Stop the goal? 3 teams stop now. Their tasks and files stay.") |
| **The summary line** | Under the header | Quiet line | "3 teams · Budget 34 % used · 2 h 05 min left", a thin meter for the budget; the exact figures in the tooltip; never "tokens" |
| **Team rows** | Under the summary | Rows | One 40 px row per team: state dot, the lead task's name ("Finance lead"), its part in one muted line, its state in words (§13b's seven, for the lead), and Open. A stuck or failed helper inside a team shows as "1 stuck" in `--danger` on its row |
| **Conversation** | The goal page's main column | The transcript's layout, with a Segmented control as the filter | Filter: All, or one team. Each message is a light row headed "Coordinator → Finance lead · 09:14", then its full text (folded with "Show all" past 12 lines) and its files as chips. A reply sits under its request, indented, headed "↳ Finance lead → Coordinator · 09:31". A person's message is headed with their name |
| **The message box** | Foot of each page | Composer | Each page's box talks to that page's agent. The goal page: placeholder "Reply to the coordinator…". A lead task's page: the quiet line under it says "The coordinator sees what you send here." A helper's page: "The lead task sees what you send here." (§13b) |
| **Waiting for you** | The asking agent's own page; the goal page; the task list | §13b's card, status and amber dot | The card appears in the conversation of the agent that asked, and is answered in that page's box. The goal page and the goal's row in the task list carry the amber dot while any agent in the goal waits; the row stays in place |
| **Lead pages** | A lead task's transcript | §13b's quiet lines | The goal messages that lead sent or received, as quiet lines: "From the coordinator: …", "To the coordinator: … · 2 files" |
| **The task list** | Sidebar | Conversation list item, as a tree | The goal is one row, its teams under it, each team's helpers under that: one line per row, chevrons at the start, counts at the end, nothing opens on its own (§13b) |
| **Stop** | Each page's ⋯ | Action menu item, then a Dialog | Stop helper (a helper's page), Stop team (a lead's page; the coordinator is told), Stop goal (the goal page) |

**States.**

| Part | Loading | Empty | Error |
|---|---|---|---|
| Goal page | Skeleton header, rows and conversation | A goal waiting for its parts to be confirmed: "Confirm the parts to start." with the proposal card | "Couldn't load the goal. Try again" |
| Team rows | Skeleton rows | Not possible once the goal starts | One row says "Couldn't load this team" with Try again; the others stay |
| Conversation | Skeleton lines | "No messages yet. The coordinator writes first." | The transcript's error line with Try again; a filter that fails keeps All |

## 13d. Computer use (17.0, ADR-0039)
A task on an environment that declares `desktop` works on a graphical desktop: the agent looks at the screen and acts
on it, and the task's owner can watch it live and take over to sign in. This section is how that looks, as the panel
ships. It follows the UI standard's controls and adds none of its own. Words are the catalogue's (`screen.*`,
`desktop.*`, `tool.computer*`, `limits.*`).

**Owner only.** A task's screen is private, like its transcript (ADR-0039 §6). Only the task's owner is offered the
Screen toggle, its tab, and its opening by itself; a person a workspace is shared with, a channel member and an
administrator never see the screen, and the page never offers it to them (the gateway refuses them too). A page that
opens it anyway says "Only the task's owner can see its screen."

| What | Where | Control | Behaviour and words |
|---|---|---|---|
| **The Screen tab** | The task page's side panel, beside Files, Changes and Context; a Screen toggle in the conversation bar | Panel tab | Only on a desktop task, for its owner. A person finds a desktop task's environment in the environment picker, where a described one reads "<description> · Desktop · <size>" ("A screen the agent works on · Desktop · 2 CPUs · 4 GB"). The tab opens by itself the first time the agent acts on the desktop, once per task; if the person then closes the panel on it, it stays closed for that task (remembered per browser). While a desktop action runs, the toggle shows a dot and its name and tip read "Screen: the agent is using the desktop", so a person with the panel closed sees it start. "Show" switches Live and Replay |
| **The live view** | Screen tab | The picture, view only | The desktop as it is now. Its status line says what's happening: "Connecting to the desktop…", "Watching. The agent is working.", "Watching.", "Reconnecting to the desktop…", "The desktop isn't running. It starts when you send a message.", "Another tab of yours has control. Hand back there, or close that tab.", "This desktop is open in four tabs. Close one to watch it here." Until the person takes over, nothing they press reaches the desktop (the gateway drops it) |
| **Take over** | Under the picture | Button (secondary) | Before it: "Taking over stops the agent. After you hand back, send a message to let it carry on." It never happens by itself. Pressed during a run, it reads "Stopping the agent before you take over…" until the run has stopped and the desktop's browser was reset; then "You have control. When you're done, hand back, then send a message to let the agent carry on." with "Keys go to the desktop while it has focus; press Escape twice to give the keyboard back to the page. You can paste in; nothing is copied out." and "A sign-in here lasts for the agent's next run only. Each run after that starts signed out." If the reset wasn't confirmed in time: "You have control. The agent may still be stopping, so a sign-in now may not last: wait until this line changes.", and the line changes once it lands. If the reset couldn't empty the browser: "Earlier sign-ins may still be in this browser. Clear them before you sign in.", with Clear sign-ins beside it. Failure: "Take over didn't go through. Try again." |
| **Hand back** | In Take over's place while in control | Button | Gives the desktop back. The panel then says "You handed back the desktop. Send a message to let the agent carry on." until a run starts, also after a reload or with the panel reopened (it's read from the screen notices). Nothing resumes by itself: the run that was stopped stays stopped, and the person's next message starts the next one |
| **Expand** | Beside Take over | Button (ghost), then a Dialog | Opens the screen wide, with the same status line, Take over or Hand back, ⋯ and notes, so a person can take over and sign in at a useful size. While the person is in control with the keyboard in the screen, one Escape goes to the desktop, Escape twice gives the keyboard back to the page, and the next one closes the dialog; its close button always closes it. Otherwise Escape closes it. Focus comes back to Expand. If the screen stops being live while expanded, the dialog closes and focus goes to the panel |
| **Clear sign-ins** | The panel's ⋯, and a button beside the earlier-sign-ins warning | Action menu item or Button, then a Dialog | "Clear the desktop browser's sign-ins?" "The browser on the desktop forgets every sign-in now. Other apps on the desktop keep theirs." Only while the agent isn't working: otherwise unavailable with "Available when the agent isn't working.", or "The browser is already signed out: each run ends that way." Outcomes in words: "The desktop browser's sign-ins were cleared.", or that they couldn't be cleared or the clearing wasn't confirmed, so they may remain, with Try again. The outcome line under the panel goes once a run starts or at the next Take over; the transcript keeps it |
| **Screen notices** | The task's transcript, in order with its turns | Quiet lines, like the team messages (§13b) | "You took over the desktop.", "You handed back the desktop after 4 min.", "The desktop was handed back when its page closed, after 12 s.", "The desktop was handed back when the server restarted." |
| **Replay** | Screen tab, Show › Replay | Slider with Previous step and Next step; Run picker | The desktop's steps run by run ("Run 2 · 14:05 · 9 steps"), each "Step 3 of 9 · <the step's line>", with Before and After pictures and "What the model saw" (its marked picture: "The numbers mark the elements the agent could act on.") |
| **Desktop step lines** | The activity group (§13) | Activity line, opening into the tool card | What was done, in words, never the tool's name or coordinates: "Looked at the screen", "Took a screenshot", "Clicked “Sign in”", "Double-clicked a table cell", "Typed 12 characters into “Email”", "Pressed ctrl+l" (the keys as the agent named them), "Scrolled down in a list", "Dragged on the screen", "Waited 2 s". An element is named by its accessible name, else by its role ("a button"). Typed text is never shown, only its length. Limits and refusals say so: "Reached this run's screenshot limit; the agent carries on without pictures", "The screen was too detailed to send as a picture", "The screen had changed; it looked again". A step's card shows its pictures first, then its output, then what it was given |
| **A Desktop's slot** | The task's status | Status pill | When the Desktop environments at once are all in use: "Waiting for a free desktop · 2 ahead" with other tasks waiting before it, else "Waiting for a free desktop: other tasks are using them all. One frees when another task has been idle for a while." (§13's waiting words) |
| **Settings › Limits › Desktop** | Settings › Limits, for administrators | Table with an Edit per row (Dialog) | Its own section, "For tasks on a Desktop environment. Each limit says when a change applies.": Desktop environments at once ("Applies at the next task start"; it can't be more than Tasks running at once, refused in those words), Keep action screenshots for ("Applies at the next clean-up"), Screenshots a run may send the model and Desktop actions a run may take (each "Applies to desktops that start after the change": a running desktop keeps the caps it started with) |

**Wording rules for computer use.**
- Say what happens to the agent's run in plain words: Take over **stops the agent**, and the person's **next message**
  lets it carry on. Never "pause", "resume" or "current step" (it's the whole run that stops).
- Never promise more than a sign-in gives: it lasts for the agent's next run only. A late grant says to wait, and its
  line changes when it's safe; a browser that couldn't be emptied says so before a sign-in.
- Typed text, keys a person presses and what's on a page are never echoed into the transcript; a typed value is a
  count of characters.
- "desktop" in prose, lowercase; "Desktop" only as the environment's or a setting's name. "Screen" is the tab.

**States.**

| Part | Loading | Empty | Error | Partial |
|---|---|---|---|---|
| Live view | "Connecting to the desktop…" | "The desktop isn't running. It starts when you send a message." | "The screen isn't available. Trying again…", after a while "…Still trying. The agent's work on the desktop isn't affected."; with no Try again: "This task's screen can't be opened.", "This task's environment has no desktop.", "Only the task's owner can see its screen."; "four tabs" offers one | "Reconnecting to the desktop…", the last picture dimmed |
| Replay | The pictures load as they come into view | "No screenshots yet." | "This screenshot isn't available any more." | "No picture at this step." or "No new picture at this step: the screen as last seen.", and "This picture couldn't be saved." where one wasn't kept |
| Take over | "Stopping the agent before you take over…" | — | "Take over didn't go through. Try again." | The late grant's line, until the reset lands |
| Clear sign-ins | "Clearing the browser's sign-ins…" | — | "…couldn't be cleared, so they may remain. Try again." | "…wasn't confirmed, so they may remain. Try again." |

## 14. Errors and resilience
- **Nothing fails silently.** Every failed request, stream error, validation error and unexpected exception on a user
  path ends in a visible message where the user is looking, with a next step. A code path that can throw on a user
  action without a visible outcome is a defect.
- Lost connection: a quiet banner "Reconnecting…" while the stream recovers, replaced by the replayed content; no
  duplicated or missing messages (WEB-U-001, WEB-U-002).
- Authentication: an expired session sends the user to sign-in and returns them to the same page afterwards; a
  sign-in attempt before any account exists explains that the first administrator must be set up and links there.
- Error boundaries around each panel: a failed panel shows its own error and a retry, and never blanks the page.
- The web owns the words: an error frame's `code` selects the web's own copy; the gateway's `message` is diagnostic
  and never shown as is (it may name internals such as "sandbox"). A failed message reads "Not sent: the workspace
  could not start" (`sandbox_failed`) or "Not sent: the agent stopped while starting" (`runtime_start_failed`), with
  **Retry**, and the text stays in the composer; an unknown code reads "Not sent: something went wrong" with Retry.

## 15. Performance budgets
Measured in the browser tests on the reference laptop profile and recorded per release (TEST-PLAN PERF-040):
- first load of the conversation page: Largest Contentful Paint ≤ 2.0 s, Interaction to Next Paint ≤ 200 ms,
  Cumulative Layout Shift ≤ 0.05;
- streaming: at least 30 frames per second while a 20 KiB answer streams; no long task over 50 ms per flush;
- JavaScript for the conversation route ≤ 250 KiB compressed; a regression of more than 10 % needs a written reason.

## 16. Browsers, screens and networks
- Supported: the current and previous major versions of Chrome, Edge, Firefox and Safari on desktop; tablets and
  phones get a usable layout (drawer navigation, full-width transcript) but are not a design target in v1.0.
- Screens from 1024 px to 2560 px wide are designed and tested; the layout never scrolls horizontally (§6).
- The app works over HTTPS and over plain HTTP on a private network address. It uses no browser API that exists only
  in a secure context (for example `crypto.randomUUID`, `navigator.clipboard`) without a tested fallback, and its
  browser tests include a run from a non-secure origin.

## 17. UX review and definition of done
A web change is done when, in addition to ENGINEERING-GUARDRAILS §9:
1. it matches this document, and the reviewer has seen it working in a real browser on the full stack, not only in
   unit tests;
2. screenshots in light and dark of every changed view, at 1280 px and at 1024 px, are attached to the report, and
   the `/design` page shows any new component in all its states;
3. loading, empty, error and partial states are shown in those screenshots or in tests;
4. keyboard-only use of the changed flow has been walked through, and the automated accessibility checks pass;
5. the flow was tried from a non-secure origin when it touches browser APIs;
6. every phase demo includes a hands-on session by the Product Owner; what they find is fixed or scheduled before the
   next gate, and recorded in DEV-PLAN.
