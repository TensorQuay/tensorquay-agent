# Take over and the agent's last page

**State:** proposed, 18 Oct 2026. **Decides:** the Product Owner (what the person sees), the technical lead (the
protocol change). **Builds on:** ADR-0039 section 6 (Take over and sign-ins).

## The problem

A person usually takes over because the agent stopped on a page that needs them: a sign-in wall, a captcha, a
confirmation step. Take over cancels the run, and the run's end resets the desktop browser: the runtime closes it,
empties its profile and starts it again on `about:blank` (ADR-0039 section 6, "Every run starts signed out" and "Take
over always finds the runtime's browser"). So the person gets control of a blank browser. The page they came for is
gone, and they have to find and type its address again before they can sign in.

The reset itself is right and stays: every run starts signed out, and a sign-in made during a Take over serves only the
run that follows. The question is only which page the fresh, signed-out browser shows.

## Where the address comes from

Snapshots don't carry it: `observe` reports the focused window's title and elements, not its address. Two sources work:

- **The browser at the reset (preferred).** Just before the reset closes the browser, the runtime asks it for the
  focused page's address, as the driver already does to pick the page it acts on (`Target.getTargets`). This is the
  page at the run's end, even when the model navigated after its last `observe`, or never observed at all.
- **The last snapshot.** Add the address to the driver's snapshot and keep the latest. This misses navigation after
  the last look, so it is the fallback only.

Only `http` and `https` addresses count. `about:blank`, the browser's own pages and `file:` addresses mean "no last
page". The address is page-controlled text: it is shown as text, never as markup, and never logged in full.

## Options

| | What happens | For | Against |
|---|---|---|---|
| **A. Keep as is** | Take over finds a blank browser. | Nothing to build. | The person retypes an address they may not know. |
| **B. Reopen at every reset** | The reset starts the browser on the last address instead of `about:blank`. | Runtime-only; `Browser::launch_at` already opens a given page. | A page is fetched again with nobody asking: one-time links (a confirmation or a password-reset link) are replayed, and the site sets fresh cookies in the profile. Every next run starts on the old page, not blank. |
| **C. Reopen at the grant** | The gateway tells the runtime when it grants Take over, and the runtime opens the last address then. | Only fetched when a person is there. | Still fetched unasked, and a protocol frame for every grant. |
| **D. Offer it** | The reset reports the last address; the person's control line offers **Open the agent's last page**, which opens it in the blank browser. | The person chooses; nothing is fetched unasked; the reset is unchanged. | One click, and a new frame for the click. |

## Recommendation: D

It gives the person the page in one click and keeps both of ADR-0039's guarantees exactly as they are: the reset still
leaves an empty profile on `about:blank`, and nothing loads that the person didn't ask for.

- **The runtime** reads the focused page's address before the run-end reset, and reports it in `screen.reset` as an
  optional `last_page`, left out when there is none (as `ready` is left out when true). Sign out everywhere offers
  nothing afterwards: the person asked for a clean browser, so the gateway drops the stored address on its
  `screen.sign_out.done`.
- **The gateway** keeps the address with the desktop session, in memory only, until the next reset. It takes the
  panel's request to open it from the Take over holder's socket only, and sends the runtime a new `screen.open_last`
  frame with its own stored address. The page never sends an address, so a viewer can't make the desktop open one.
- **The runtime** navigates the blank browser to the address, through the sandbox's egress rules as any page is.
- **The panel** shows the line under the control line: "The agent was on example.org/login. **Open it here**". It
  shows the host and path, shortened, as text. After the click the line goes away.

**What the person sees:** they press Take over; the control line says they have control, with the agent's last page
offered under it. They open it and the page loads signed out, because the profile is still empty. They sign in, hand
back and send a message; the next run starts with that sign-in, as today.

This adds a client frame and a `screen.reset` field, so it needs a runtime protocol revision (ADR-0026).

## Tests

| Level | Test |
|---|---|
| Runtime unit | The run-end reset records the focused page's address before it closes the browser, with a fake browser. There is none for `about:blank`, `chrome:` and `file:` pages. The profile is still emptied after it. |
| Protocol | `screen.reset` round-trips `last_page`, which is left out when absent, and a frame without it still parses. `screen.open_last` is in the new revision's list (`revision.rs`). |
| Gateway integration | The request to open the last page is acted on only from the Take over holder. It is refused from a view-only socket, after Hand back and when no address is stored. Sign out everywhere clears the address. |
| Web unit | The line shows the shortened address as text, never as a link or markup. The button sends the frame once and the line goes away. |
| End to end (Desktop) | The agent opens a page on the fixture site and the run ends. Take over offers that page, and opening it shows the page signed out (the fixture's session cookie is absent). The existing tests that every run starts signed out still pass unchanged. |
