# TensorQuay Agent web

Next.js App Router client for the gateway's single public origin (ADR-0022). `/setup` creates and signs in the first administrator,
`/login` redirects fresh installations to setup and otherwise establishes a session, `/` opens an empty composer, and `/c/[id]` replays and streams a conversation.
The sidebar lists conversations, marks the active one and shows relative activity times. The first send creates the
conversation, derives its title from the first line (at most 60 characters), then sends the prompt with a separate
idempotency key. An uncertain create or send keeps its key and the draft for an explicit retry. A definitive execution failure restores the draft and Retry creates a new action. The prompt appears
immediately while requests are pending; client navigation preserves it until the stream confirms the message.
The conversation bar shows a renamable title, model display name and human run status. Model changes use a keyed metadata update and are disabled during work with a reason. The composer contains only the message and Send/Sending/Stop. Three example tasks fill an empty composer without creating a conversation. Tab titles follow the selected conversation; the account menu holds identity, theme and sign out.
`/design` is a development and visual-test reference; normal production builds return 404 and the navigation omits it.
Private data is fetched only in the browser;
the gateway strips credentials before forwarding page requests to Next.js.
A wrong setup token (`unauthorized`) shows web-owned copy pointing to the installer output; the gateway message is never shown.
Explicit sign-out suppresses expiry redirects from late requests, including during navigation to `/login`.
A failed sign-out or successful sign-in restores ordinary expiry redirects with the local return path;
responses from a previous session cannot redirect the new session.

## Stack

Node 22.13.0 or newer is required; `.nvmrc` records Node 24.21.0, the version the web image builds on.
Installs reject unsupported Node versions through `engine-strict=true`. The lockfile was
created with pnpm 10.34.5, pinned through Corepack in `packageManager`.
Every direct dependency is pinned to an exact version. ADR-0017 excludes the
optional native image dependency; image optimisation is disabled.

| Package                             | Version |
| ----------------------------------- | ------- |
| Next.js / eslint-config-next        | 16.4.1  |
| React / React DOM                   | 19.2.8  |
| Ajv                                 | 8.20.0  |
| TanStack Query                      | 5.103.2 |
| Tailwind CSS / @tailwindcss/postcss | 4.3.3   |
| TypeScript                          | 5.9.3   |
| @types/node                         | 24.19.0 |
| @types/react / @types/react-dom     | 19.3.0  |
| ESLint                              | 9.39.5  |
| typescript-eslint                   | 8.70.1  |
| eslint-plugin-react-hooks           | 7.1.1   |
| eslint-plugin-jsx-a11y              | 6.10.2  |
| eslint-plugin-import                | 2.32.0  |
| Prettier                            | 3.9.8   |
| Vitest                              | 4.1.11  |
| @testing-library/react              | 16.3.3  |
| @testing-library/jest-dom           | 6.9.1   |
| jsdom                               | 29.1.1  |
| @playwright/test                    | 1.63.0  |
| @axe-core/playwright                | 4.13.0  |
| lucide-react                        | 1.47.0  |
| react-markdown                      | 10.1.0  |
| remark-gfm                          | 4.0.1   |

Vitest 4 and jsdom 29 support the recorded Node version. Their next major
releases require a newer Node version.

## Commands

Run these commands from `web/`:

| Command                                          | Purpose                                                                                                                                 |
| ------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| `corepack pnpm install --frozen-lockfile`        | Install the locked dependency tree.                                                                                                     |
| `corepack pnpm run dev`                          | Start the development server.                                                                                                           |
| `corepack pnpm run format:check`                 | Check formatting.                                                                                                                       |
| `corepack pnpm run format`                       | Apply formatting.                                                                                                                       |
| `corepack pnpm run lint`                         | Run Next.js, typed TypeScript, hooks, accessibility and import-order rules.                                                             |
| `corepack pnpm run typecheck`                    | Run `tsc --noEmit` with strict compiler checks.                                                                                         |
| `corepack pnpm run tokens`                       | Reject colour, font-family, radius and shadow literals outside the token stylesheet.                                                    |
| `corepack pnpm run api:check`                    | Check that every gateway call comes from the operations table and is in the OpenAPI document (GW-U-027).                                |
| `corepack pnpm run perf:transcript`              | PERF-040/WEB-E-009: a 10,000-event transcript renders within 2 s and scrolls smoothly.                                                  |
| `node scripts/check-tokens.mjs --self-test`      | Verify the token rule with compliant and forbidden fixtures.                                                                            |
| `corepack pnpm run test`                         | Run unit and component tests once.                                                                                                      |
| `corepack pnpm run licenses`                     | Check installed package license declarations against the allow list.                                                                    |
| `node scripts/check-licenses.mjs --self-test`    | Verify license parsing and package traversal with embedded fixtures.                                                                    |
| `node scripts/check-licenses.mjs --notices`      | List installed packages requiring MPL-2.0 or CC-BY-4.0 notices.                                                                         |
| `corepack pnpm run build`                        | Build the production application.                                                                                                       |
| `corepack pnpm run start`                        | Start the production application after building.                                                                                        |
| `corepack pnpm run check`                        | Run protocol freshness, format, lint, typecheck, tokens, tests, licenses, build and production-route checks; stop at the first failure. |
| `corepack pnpm run test:production`              | Check the default build returns 404 for the development reference and omits its navigation link.                                        |
| `corepack pnpm exec playwright install chromium` | Install the browser used by the accessibility, layout and screenshot tests.                                                             |
| `corepack pnpm run test:e2e`                     | Build with the visual-test reference enabled and run the Chromium suite.                                                                |
| `corepack pnpm audit --audit-level=high`         | Report dependency advisories.                                                                                                           |

The browser suite starts Next.js privately on port 3101 and a fake gateway on port 3100. The fake serves APIs and
SSE streams and forwards credential-free pages; every emitted frame is checked against the real JSON Schema.
Each test has a separate session and conversation history. CI also installs Chromium's system dependencies.
The test build sets `TQA_WEB_TEST_BUILD=1` solely to include `/design`; normal builds and the container do not set it.
HTTP-origin tests discover a non-loopback IPv4 interface and fail if none exists. The synthetic front listens on IPv4
interfaces so these tests exercise real browser restrictions, including secure-random keys and copying code.

`globals.css` owns the colour, typography, spacing and radius tokens. The shell
uses the system theme until a choice is stored; storage failures leave the
current tab usable. Shared primitives live in `src/ui/`; component and keyboard
tests accompany them. WEB-U-007 covers computed contrast and browser accessibility.

The development, build and browser-test hooks copy the SVGs, fonts and licence
texts from `brand/` into the ignored `public/brand/` directory. Edit the source
assets only. Fonts are served locally; no external font service is used.

Screenshot baselines live in `e2e/__screenshots__/{darwin,linux}/` because font
rasterisation differs across operating systems. Review both themes before
accepting a visual change. To regenerate the baselines on the current platform,
run `corepack pnpm run test:e2e --update-snapshots`; normal test runs compare
against those files without updating them.
Linux references come from `scripts/linux-baselines.sh`, which runs the official Playwright container pinned by digest
(test tooling only, never shipped): `scripts/linux-baselines.sh design` updates the design references against the fake
front, and `scripts/linux-baselines.sh ux STACK_URL TOKEN_FILE OUTPUT_DIR` captures `ux-*` candidates from a stack started
with `deploy/tests/demo.py --suite ux --keep` (the URL and token file it prints). Review candidates before copying them into
`e2e/__screenshots__/linux/`. Linux dependencies and build output stay in named Docker volumes.

WEB-E2E-018 runs separately on the real deployment through `deploy/tests/demo.py --suite ux`.
It covers a non-secure LAN origin at 1024, 1440 and 1920 pixels in both themes, two conversations per case,
automatic sign-in after setup, live follow and pause, Markdown, narration folding, singular and plural summaries, fixed layout, human status, conversation-bar controls, tab titles, exact replay and console output. Application chrome is checked for internal state words, including failed-message copy in the fake-front browser suite.
The visual checks crop the start, live activity, folded turn and Markdown views at 1440 pixels; taller capture
viewports keep the activity and Markdown examples visible. Only recorded durations and relative times are masked.
The tolerance is 0.1% changed pixels with a per-pixel threshold of 0.1. Normal runs never update references.
For review, `TQA_UX_CAPTURE=1` writes candidate images to `TQA_WEB_EVIDENCE_DIR` without comparing them.
Keep the original candidates in private evidence for review and commit references only after that review.
`TQA_UX_BASELINES` can select a separate reference directory for verification; actual captures never overwrite references.
WEB-E2E-019 reuses the same assertions with an explicitly supplied private model route and coding prompt;
model-dependent content has no screenshot baseline. Credentials and the smoke entry point stay outside the repository.

`licenses.json` defines the allow and forbidden lists from ADR-0017. A forbidden
branch fails even within an `OR` expression. pnpm omits the optional `sharp`
dependency and its native packages through `ignoredOptionalDependencies`;
Next.js image optimisation stays disabled. Install with the pinned package
manager so the same licence policy applies locally and in CI.

## Protocol and live state

Run `corepack pnpm protocol:gen` after changing the shared protocol schema or gateway OpenAPI. `protocol:check`
fails on stale types or runtime schemas, including constraint-only changes. Tagged unions remain discriminated;
the reducer's exhaustive `never` checks make a new event variant a compile error until it is handled.
The pure `present` layer independently assigns every event one treatment: answer, prompt, activity, thinking,
status, summary or hidden. Each turn contains one activity group above its Markdown answer. Running work stays
open; text from tool steps stays inside the group before those tools and folds with the work. Streaming text stays there until its step resolves; only the last completed step without tools becomes the answer. Completion folds the work, while failure or cancellation keeps the group open with a next step. Builtin tool
summaries use recorded arguments and results, cap lines at 180 characters and preserve both ends of long values.
Unknown tools reveal only their names. Search summaries distinguish match counts from matching-file counts and
mark partial results. Each activity opens the full tool card; thinking remains a collapsed disclosure.
Finished summaries use recorded timestamps, authoritative steps, unique changed paths, executed commands and usage,
so a replay produces the same content.
An initial environment event before any conversation activity is hidden. Later `environment.changed` events become
quiet "Switched to <template>" status lines, including the first recorded switch after work in the implicit environment.
They use the catalogue's display name with the recorded identity as a fallback for removed templates.
The environment picker lists declared capabilities beside the model picker.
Before the first message it selects the create request's template;
later it asks for inline confirmation and makes an idempotent switch while idle. The selected value changes only
on the server's success response; the transcript line waits for the runtime event on the next run. Sending is
disabled while a switch is pending, with the draft preserved. WEB-U-018 covers selection, cancellation, active-run
refusal, uncertain-delivery retry and replay. E2E-ENV-010 exercises both images through the demo driver.

Ajv validates network frames and API responses before they enter application state. API requests have a 10-second
deadline, a 2 MiB response cap and cancellation. Actions keep one UUID idempotency key for retries after uncertain delivery. Definitive action failures map stable codes to web-owned copy, keep the draft and use a new key for Retry; gateway diagnostic messages never appear verbatim. For uncertain delivery, automatic retries are limited to three attempts on 429 with a Retry-After of at most 10 seconds.
Generated browser schemas omit documentation annotations to keep the conversation route within its compressed
JavaScript budget. Validation constraints and constant data are retained; the original schema documentation and
the generated type fingerprint continue to cover the full source contract.
Title editing, the account popover, navigation dialogs, confirmations and full tool details load when opened. The header and
new-conversation link leave the start screen unloaded until navigation, keeping its composer out of a conversation's initial download.

The transcript applies frames immediately and publishes at most once per animation frame. Initial EventSource
connections replay from the beginning. Native reconnects use Last-Event-ID; a detected gap opens a fresh stream
with `after=<last applied sequence>`. Duplicate sequences do not change the view, and a higher step attempt
replaces its partial text, reasoning and calls. Assistant answers render through `react-markdown` and `remark-gfm`
with an explicit element allow-list. Raw HTML and images remain text; links allow only HTTP, HTTPS and mailto.
Code blocks have language labels, horizontal scrolling and copy controls with a plain HTTP fallback. Reasoning and
tool output stay plain text. No HTML parser or raw-HTML React escape hatch is used.
Only status changes and errors are announced; tokens have no live region. Markdown loads when recorded text first
needs it; following also tracks content resizing so the late renderer preserves the reader's position.

WEB-U-003 covers inline approval requests. A pending request focuses its card and shows "Waiting for your approval"
in the conversation bar. Allow and Deny, including the card-local A and D shortcuts, echo the recorded action hash;
uncertain delivery retries the same decision and idempotency key. An optional `session_grant` adds a secondary
"Allow <scope> for this task" action (S). The scope appears
verbatim in code type: the supplied pattern, or the complete exact argument object. Acceptance and replay read
"Allowed for this task". A request without the scope never offers this action. The quiet countdown comes from `expires_at` and
never authorizes execution. It reads "Expires in 4:59" under an hour and "Expires in 23 h 59 min" from an hour ("Expires in 1 h" on the hour); a
polite live region repeats it in whole minutes, so announcements change at most once a minute. Recorded decisions and expiry become quiet status lines on the card, with identical
replay. A binding refusal offers one fixed recovery line without exposing gateway diagnostics. Legacy requests
without a hash cannot submit decisions. The approval code loads only when a request needs rendering; the fake-front
UX suite exercises all three decisions, replay, expiry and refusal over a non-secure origin.
The separate `deploy/tests/demo.py --suite approvals` contract runs the same decisions through the real gateway
and runtime with a narrow administrator Ask rule. It independently observes the recorded action hash, verifies
replay and refusal responses, and checks the resulting workspace on the host. Its expiry case uses the default
five-minute lifetime. Set `TQA_WEB_EVIDENCE_DIR` to retain its light/dark captures.

A live `egress_denied` error becomes one quiet line, "Blocked network access to <host>:<port> - it is not on the
allow-list.", at most once per destination per minute by the store's clock; it is never stored, so a reload shows only the
denied `fetch_url` activity line, which uses the same words. Repository clones read "Cloning the repository…",
"Repository cloned" or "Clone failed: <reason>" from their stable code; a skipped clone is hidden.
The shell fills the viewport. The conversation scrolls inside its own column and keeps the composer visible.
Admission updates use unsequenced `session.ready` metadata. The bar shows "Waiting for a free slot · 2 ahead"
or "· next" at zero, then "Starting…" while the environment wakes. Queue positions never become transcript
items or move the replay cursor. Accepted queued messages leave "Sending…", retain their prompt, and disable
the composer with the one line "Your message is queued." until admitted. After work, idle or destroyed environments
explain "Asleep, wakes when you send"; a parked approval reads "Paused until you decide". An empty new task stays "Ready".
`deploy/tests/demo.py --suite queue` exercises real admission with one slot, an approval holding the active run,
queued reloads, and a second queued turn over prior idle history. The fake matrix also verifies changing positions.
Within 24 pixels of the bottom, each published sequence follows the latest content; scrolling away pauses it.
"Jump to latest" shows a count only for positive new sequenced updates since the pause, and jumping, scrolling back or sending resumes it.
All follow movements are immediate, including when reduced motion is requested.
The composer is one paper card containing the message and a round Send/Sending/Stop control. The model picker is a keyboard-operable menu in the conversation bar above the transcript. Its disabled explanation appears on hover or focus. Enter sends,
Shift+Enter adds a line, and Stop requires a keyboard-operable confirmation. Empty composers show keyboard hints;
The composer clears immediately on send, with the prompt shown once above it; a failed send restores the draft. Sending is conveyed by the round button. Other disabled states explain their reason inside the card. The empty start page centres the greeting, composer and example tasks. Account preferences open beside the account icon without an overlay. The design reference includes these control and activity
states in both themes.

## Run through the gateway

Start Next.js on the internal page-server port, configure the gateway, and open the gateway's public origin:

```toml
[web]
upstream = "http://127.0.0.1:3000"
dev = true
```

`dev` defaults to false; it enables the development hot-reload path `/_next/hmr` and emits a gateway startup warning.
The production image uses the standalone output with no secrets and runs as a non-root account. See
[the deployment guide](../deploy/README.md). Only the gateway port is published.

## Languages

Every visible string comes from a catalogue, one file per language in `src/i18n/locales/<code>.json` (ICU message
format; UI-GUIDELINES §11b). English (`en.json`) is the reference. Another catalogue may translate some of its messages:
any message it leaves out shows in English, never as a key.

- `corepack pnpm i18n:gen` discovers the catalogues and writes `src/i18n/generated/languages.ts`: each language's
  code, its name in its own script, and whether it is **complete** (it translates every reference message).
  It also splits English by use into `en.core.json`, which every page loads, and `en.settings.json`, which only the
  settings pages load: a message moves to the settings part when only settings files (that no other page imports)
  name it. Translators keep editing one file per language; the split is generated and checked.
  `corepack pnpm i18n:check` fails when that file is stale, so completeness always matches the catalogues.
- The language menu labels a language that is not complete "(partial)", in the reader's language (WEB-U-032). The
  label disappears by itself once the catalogue is complete and `i18n:gen` has run. There is no hand-kept list.
- `corepack pnpm i18n:report` lists, per language, how many messages are translated and which keys are missing or
  unknown. It never fails the build.
- A catalogue marked `"status": "draft - native review welcome"` still needs a native speaker's review. The review
  pull request removes the mark. "Draft" and "partial" are separate: a draft can be complete, and a reviewed
  catalogue can still miss newer messages.

To add or finish a language, follow [Add a language](../CONTRIBUTING.md#add-a-language).

## Wording and polish

The product calls the unit of work a **task** in every user-visible string, accessible name and tab title
(UI-GUIDELINES §10); the API, protocol, route paths and code identifiers keep "conversation". WEB-U-021
(`e2e/polish-audit.spec.ts`, helpers in `e2e/ux/polish-audit.ts`) audits every main view at 1024, 1440 and 1920 pixels in
both themes: computed paddings, margins and gaps on the spacing scale, radii from the token set, controls at least 28 pixels
high with a pointer (or `not-allowed` when disabled), a visible focus ring on every Tab stop, a visible hover change on every
enabled control, no text overflowing its box, one icon size per context, one primary button per view, and no visible
"conversation". `pnpm run tokens` also rejects spacing literals outside the `--space-*` scale.
