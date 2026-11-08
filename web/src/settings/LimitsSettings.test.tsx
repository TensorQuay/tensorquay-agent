import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { User } from "../protocol/generated/api";
import { cellNote } from "../test/cell-note";
import {
  DESKTOP_LIMITS,
  fakeLimits,
  limit,
  LIMITS as ADMIN_LIMITS,
  TEAM_LIMITS,
} from "../test/limits";
import { choose, chosen } from "../test/picker";
import { LIMITS } from "../test/user";

import type { LimitsApi } from "./admin/limits";
import { LimitsSettings } from "./LimitsSettings";

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
function show(api: LimitsApi, me: User = admin) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <LimitsSettings me={me} api={api} />
    </QueryClientProvider>,
  );
}
const row = (name: string) =>
  screen.getByRole("row", { name: new RegExp(name) });
const dialog = () => screen.getByRole("dialog");
const enter = (value: string) =>
  fireEvent.change(within(dialog()).getByRole("spinbutton"), {
    target: { value },
  });
const save = () =>
  fireEvent.click(within(dialog()).getByRole("button", { name: "Save limit" }));
const edit = (name: string) =>
  fireEvent.click(screen.getByRole("button", { name: `Edit ${name}` }));

// Slice 5 (ADR-0028): admission and sandbox limits for administrators; floors and adjusted values shown (WEB-U-041).
describe("WEB-U-041 limits", () => {
  // Three columns (UI-GUIDELINES §7): when a change applies is in the page's sentence, the range
  // and the default are in the edit dialog, and the source shows only for a value set here.
  it("lists every limit and its value, and says only what differs from the configuration file", async () => {
    show(fakeLimits().api);
    await screen.findByRole("table", { name: "Limits" });
    expect(
      screen.getAllByRole("columnheader").map((cell) => cell.textContent),
    ).toEqual(["Limit", "Value", "Actions"]);
    const slots = row("Tasks running at once");
    expect(within(slots).getByText("8 tasks")).toBeVisible();
    expect(slots).not.toHaveTextContent(/Default|Next task start|Set here/);
    const idle = row("Sleep after idle");
    expect(within(idle).getByText("15 min")).toBeVisible();
    expect(within(idle).getByText("Set here")).toBeVisible();
    // Sizes are each environment's now (the owner's direction): no CPU, memory or process rows here.
    expect(
      screen.queryByText(
        /CPU per environment|Memory per environment|Processes per environment/,
      ),
    ).toBeNull();
    // PRD FR-10, superseded: nothing waits for an approval, so there is no park limit.
    expect(screen.queryByText(/Park a waiting run/)).toBeNull();
    expect(
      within(row("Connector calls per person per minute")).getByText(
        "120 calls/min",
      ),
    ).toBeVisible();
    expect(row("Connector calls per person per minute")).not.toHaveTextContent(
      /Configuration file changed|^Configuration file$/,
    );
    // Rows hold one action, Edit, so the column stays aligned; a reset is in the edit dialog.
    expect(screen.queryAllByRole("button", { name: /^Reset / })).toEqual([]);
  });

  // The configuration file's ceiling for environments, read-only: sizes are set on each environment, never above it.
  it("shows the largest size an environment may have, from the configuration file", async () => {
    show(fakeLimits().api);
    const largest = await screen.findByRole("region", {
      name: "Largest environment",
    });
    // Thousands take a narrow no-break space (UI-GUIDELINES §10).
    expect(largest.querySelector("p")).toHaveTextContent(
      "Environments can use up to 8 CPUs, 16 GB of memory, 4\u202f096 processes and 100 GB of disk. The configuration file sets this; each environment's own size is on Environments.",
      { normalizeWhitespace: false },
    );
    expect(
      within(largest).getByRole("link", { name: "Environments" }),
    ).toHaveAttribute("href", "/settings/environments");
  });

  it("puts the range, the default and where the value comes from in the edit dialog", async () => {
    show(fakeLimits().api);
    await screen.findByRole("table", { name: "Limits" });
    edit("Connector calls per person per minute");
    // Thousands take a narrow no-break space (UI-GUIDELINES §10).
    expect(within(dialog()).getByText(/Default 60 calls\/min/)).toBeVisible();
    expect(within(dialog()).getByText(/Set here/)).toBeVisible();
    fireEvent.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    edit("Longest environment life");
    expect(
      within(dialog()).getByText(/Configuration file changed since/),
    ).toBeVisible();
  });

  // At load the gateway moves a stored value the file's bounds no longer allow to the nearest bound and keeps the old
  // one in clamped_from: the row shows the bound, and the note says what it was. Memory has no ceiling, so never moves.
  it("says in words what the gateway moved into range, or what the file set below what can be set here", async () => {
    show(fakeLimits().api);
    await screen.findByRole("table", { name: "Limits" });
    const slots = row("Tasks running at once");
    expect(within(slots).getByText("8 tasks")).toBeVisible();
    expect(
      within(slots).getByText(
        "Lowered from 12 tasks when the configuration file changed; 8 tasks is the most allowed.",
      ),
    ).toBeVisible();
    expect(
      within(row("Running tasks per person")).getByText(
        "The configuration file sets 1 task, below the 2 tasks that can be set here.",
      ),
    ).toBeVisible();
    expect(screen.queryByText(/asks for/)).toBeNull();
    expect(screen.queryByText("Below the usual minimum")).toBeNull();
  });

  // 17.3: the list keeps a limit to two lines (a long note whole in the line's tooltip); the limit's own place,
  // its Edit dialog, says the note in full.
  it("says the same note in the limit's Edit dialog", async () => {
    show(fakeLimits().api);
    await screen.findByRole("table", { name: "Limits" });
    edit("Tasks running at once");
    expect(
      within(dialog()).getByText(
        "Lowered from 12 tasks when the configuration file changed; 8 tasks is the most allowed.",
      ),
    ).toBeVisible();
    fireEvent.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    edit("Running tasks per person");
    expect(
      within(dialog()).getByText(
        "The configuration file sets 1 task, below the 2 tasks that can be set here.",
      ),
    ).toBeVisible();
  });

  it("offers a reset in the edit dialog only for a value set here, confirms it and reports the value", async () => {
    show(fakeLimits().api);
    await screen.findByRole("table", { name: "Limits" });
    edit("Tasks running at once");
    expect(
      within(dialog()).queryByRole("button", { name: /^Reset/ }),
    ).toBeNull();
    fireEvent.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    edit("Sleep after idle");
    fireEvent.click(
      within(dialog()).getByRole("button", {
        name: "Reset to the configuration file's value",
      }),
    );
    const confirm = screen.getByRole("dialog", {
      name: "Reset Sleep after idle to 20 min?",
    });
    fireEvent.click(
      within(confirm).getByRole("button", { name: "Reset limit" }),
    );
    expect(
      await screen.findByText("Reset Sleep after idle to 20 min."),
    ).toBeVisible();
  });

  // A duration is edited as a number and a unit, preset to the unit the table shows, with the range in that unit;
  // minutes and hours take decimals and are saved in whole seconds.
  it("edits a duration in the unit shown, with a unit choice and the range in that unit", async () => {
    const fake = fakeLimits();
    show(fake.api);
    await screen.findByRole("table", { name: "Limits" });
    edit("Sleep after idle");
    expect(within(dialog()).getByLabelText("Value")).toHaveValue(15);
    expect(chosen("Unit", dialog())).toBe("Minutes");
    expect(within(dialog()).getByText("From 1 to 1440.")).toBeVisible();
    enter("0.5");
    save();
    expect(
      await within(dialog()).findByText("Enter a number from 1 to 1440."),
    ).toBeVisible();
    expect(fake.writes).toEqual([]);
    enter("90");
    choose("Unit", "Hours", dialog());
    expect(within(dialog()).getByText("From 0.02 to 24.")).toBeVisible();
    save();
    expect(
      await screen.findByText("Saved Sleep after idle: 90 min."),
    ).toBeVisible();
    expect(fake.writes).toEqual([
      { key: "sandbox.idle_seconds", value: 5400, version: 3 },
    ]);
    edit("Longest environment life");
    choose("Unit", "Seconds", dialog());
    expect(chosen("Unit", dialog())).toBe("Seconds");
    expect(within(dialog()).getByLabelText("Value")).toHaveValue(14400);
    enter("4.5");
    save();
    expect(
      await within(dialog()).findByText(
        "Enter a whole number from 600 to 604800.",
      ),
    ).toBeVisible();
  });

  // Changing the unit converts the value, so 90 minutes never silently becomes 90 hours; the text holds at most four
  // decimals, which still saves the same whole seconds.
  it("converts the value when a duration's unit changes", async () => {
    const fake = fakeLimits();
    show(fake.api);
    await screen.findByRole("table", { name: "Limits" });
    edit("Sleep after idle");
    const field = within(dialog()).getByLabelText("Value");
    enter("90");
    choose("Unit", "Hours", dialog());
    expect(field).toHaveValue(1.5);
    choose("Unit", "Seconds", dialog());
    expect(field).toHaveValue(5400);
    enter("50");
    choose("Unit", "Hours", dialog());
    expect(field).toHaveValue(0.0139);
    choose("Unit", "Minutes", dialog());
    expect(field).toHaveValue(0.8333);
    choose("Unit", "Seconds", dialog());
    expect(field).toHaveValue(50);
    // Nothing to convert: a blank or unreadable value stays as typed.
    enter("");
    choose("Unit", "Minutes", dialog());
    expect(field).toHaveValue(null);
    enter("90");
    choose("Unit", "Seconds", dialog());
    expect(field).toHaveValue(5400);
    save();
    expect(
      await screen.findByText("Saved Sleep after idle: 90 min."),
    ).toBeVisible();
    expect(fake.writes).toEqual([
      { key: "sandbox.idle_seconds", value: 5400, version: 3 },
    ]);
  });

  // The gateway sends i64::MAX for a limit with no ceiling (memory, limits.rs MAX_MEMORY_BYTES): JSON reads it as 2^63,
  // which is a whole number of GB, so it once read "256 MiB to 8589934592 GB".
  it("names the gateway's reasons, including a floor the page did not know", async () => {
    show(fakeLimits({ floors: { "sandbox.idle_seconds": 600 } }).api);
    await screen.findByRole("table", { name: "Limits" });
    edit("Running tasks per person");
    enter("9");
    save();
    expect(
      await within(dialog()).findByText(
        "Running tasks per person must not be more than tasks running at once.",
      ),
    ).toBeVisible();
    fireEvent.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    edit("Sleep after idle");
    enter("5");
    save();
    expect(
      await within(dialog()).findByText(
        "This value is outside what the gateway allows for this limit.",
      ),
    ).toBeVisible();
  });

  it("asks to reload after another administrator's change, then saves with the new version", async () => {
    const fake = fakeLimits();
    show(fake.api);
    await screen.findByRole("table", { name: "Limits" });
    edit("Running tasks per person");
    enter("3");
    fake.touch("admission.per_user");
    save();
    expect(
      await within(dialog()).findByText(
        "Someone changed this limit since you opened it. Reload to see the current value.",
      ),
    ).toBeVisible();
    fireEvent.click(within(dialog()).getByRole("button", { name: "Reload" }));
    // The reload is done once the conflict message clears and the current value is back in the field.
    await waitFor(() =>
      expect(
        within(dialog()).queryByText(/Someone changed this limit/),
      ).toBeNull(),
    );
    enter("3");
    save();
    expect(
      await screen.findByText("Saved Running tasks per person: 3 tasks."),
    ).toBeVisible();
    expect(fake.writes.map((write) => write.version)).toEqual([1, 2]);
  });

  it("says plainly when the limits cannot be loaded", async () => {
    show({
      ...fakeLimits().api,
      limits: () => Promise.reject(new Error("offline")),
    });
    expect(
      await screen.findByText("The limits could not be loaded."),
    ).toBeVisible();
  });

  it("is for administrators only", () => {
    show(fakeLimits().api, { ...admin, role: "member" });
    expect(
      screen.getByText("Only administrators can manage limits."),
    ).toBeVisible();
  });
});

// ADR-0034 S6: the helper limits are runtime rows like the others, in their own section; the gateway lists them only
// while agent teams are on, so with teams off the page has no trace of them (WEB-U-228).
describe("WEB-U-227 the helper limits", () => {
  // Room for a lead task and its 3 helpers: 4 at once, 4 per person.
  const ROOM = [
    limit("admission.slots", { value: 4, default: 4 }),
    limit("admission.per_user", { value: 4, default: 4 }),
  ];
  const helpers = () => screen.getByRole("table", { name: "Helper limits" });
  const helperRow = (name: string) =>
    within(helpers()).getByRole("row", { name: new RegExp(name) });

  it("lists them in their own section, with the switch read-only and each value in words", async () => {
    show(fakeLimits({ limits: [...ROOM, ...TEAM_LIMITS] }).api);
    await screen.findByRole("heading", { name: "Helpers" });
    const on = helperRow("Tasks may lead helpers");
    expect(on).toHaveTextContent("On");
    expect(on).toHaveTextContent(
      "Set in gateway.toml; a change needs a restart.",
    );
    expect(within(on).queryByRole("button")).toBeNull();
    expect(helperRow("Helpers per lead task")).toHaveTextContent(
      "A helper that's done still counts until it's stopped.",
    );
    for (const [name, value] of [
      ["Helpers per lead task", "3 helpers"],
      // The page reads narrow no-break space thousands (limit-copy.test.ts); the query normalises them to spaces.
      ["Tokens per team", "2 000 000 tokens"],
      ["Time per team", "2 h"],
      ["Tell the lead a helper is idle after", "10 min"],
      ["Tell the lead a helper is stuck after", "15 min"],
      ["Stop an idle team after", "30 min"],
    ] as const) {
      expect(within(helperRow(name)).getByText(value)).toBeVisible();
    }
    // Not in the general table as well.
    expect(
      within(screen.getByRole("table", { name: "Limits" })).queryByText(
        /Helpers per lead task|Tokens per team/,
      ),
    ).toBeNull();
    expect(
      screen.getByText(
        "For helpers and their teams. Each limit says when a change applies.",
      ),
    ).toBeVisible();
  });

  // Each row says when its change applies, as the Desktop's do (the gateway's `applies`: next_member, next_team,
  // next_check), and its edit dialog says the whole rule.
  it("says under each value when a change applies, and the whole rule where it's changed", async () => {
    show(fakeLimits({ limits: [...ROOM, ...TEAM_LIMITS] }).api);
    await screen.findByRole("heading", { name: "Helpers" });
    for (const [name, hint] of [
      ["Helpers per lead task", "Applies to the next helper"],
      ["Tokens per team", "Applies to the next team"],
      ["Time per team", "Applies to the next team"],
      ["Tell the lead a helper is idle after", "Applies within seconds"],
      ["Tell the lead a helper is stuck after", "Applies within seconds"],
      ["Stop an idle team after", "Applies within seconds"],
    ] as const)
      // In the value's one muted line (17.3).
      expect(cellNote(helperRow(name), hint)).toBeVisible();
    expect(screen.queryByText(/next team, helper or check/)).toBeNull();
    for (const [name, whole] of [
      [
        "Helpers per lead task",
        "A change applies from a lead's next helper; helpers already working keep going.",
      ],
      [
        "Tokens per team",
        "A change applies to the next team a lead starts; a team already running keeps its budget and time.",
      ],
      [
        "Stop an idle team after",
        // WEB-U-376 (a live check before 17.0): what a person can act on, not "the gateway's next check".
        "A change applies within seconds, to teams already running too.",
      ],
    ] as const) {
      edit(name);
      expect(within(dialog()).getByText(whole)).toBeVisible();
      fireEvent.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    }
  });

  it("says when fewer places than the limit leave room for helpers", async () => {
    show(
      fakeLimits({
        limits: [
          limit("admission.slots", { value: 3, default: 3 }),
          limit("admission.per_user", { value: 4, default: 4 }),
          ...TEAM_LIMITS,
        ],
      }).api,
    );
    await screen.findByRole("heading", { name: "Helpers" });
    expect(helperRow("Helpers per lead task")).toHaveTextContent(
      "At most 2 helpers now: Tasks running at once is 3 tasks, and the lead task takes one place. A helper that's done still counts until it's stopped.",
    );
  });

  it("edits one in its unit and names the order the gateway keeps", async () => {
    show(fakeLimits({ limits: [...ROOM, ...TEAM_LIMITS] }).api);
    await screen.findByRole("heading", { name: "Helpers" });
    edit("Helpers per lead task");
    expect(within(dialog()).getByLabelText("Value (helpers)")).toBeVisible();
    enter("4");
    save();
    expect(
      await within(dialog()).findByText(
        "Helpers per lead task must be less than Tasks running at once and Running tasks per person: the lead task takes one place.",
      ),
    ).toBeVisible();
    enter("2");
    save();
    expect(
      await screen.findByText("Saved Helpers per lead task: 2 helpers."),
    ).toBeVisible();
  });

  it("shows the helpers' model and check, and saves a model with the version read", async () => {
    const fake = fakeLimits({ limits: [...ROOM, ...TEAM_LIMITS] });
    show(fake.api);
    const model = await within(
      await screen.findByRole("table", { name: "Helper limits" }),
    ).findByRole("row", { name: /Helpers' model/ });
    expect(model).toHaveTextContent("The lead task's model");
    expect(helperRow("Check before a team finishes")).toHaveTextContent("None");
    edit("Helpers' model");
    expect(chosen("Helpers' model", dialog())).toBe("The lead task's model");
    choose("Helpers' model", "Local model", dialog());
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
    expect(
      await screen.findByText("Saved Helpers' model: Local model."),
    ).toBeVisible();
    expect(fake.teamWrites).toEqual([
      { member_route: "local", acceptance_check: null, version: 0 },
    ]);
    expect(helperRow("Helpers' model")).toHaveTextContent("Local model");
  });

  it("says when a model was deleted since the list, and when someone else changed the settings", async () => {
    const fake = fakeLimits({ limits: [...ROOM, ...TEAM_LIMITS] });
    show(fake.api);
    await screen.findByRole("row", { name: /Helpers' model/ });
    edit("Helpers' model");
    choose("Helpers' model", "Retired model", dialog());
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
    expect(
      await within(dialog()).findByText(
        "This model route no longer exists. Choose another.",
      ),
    ).toBeVisible();
    fake.touchTeams();
    choose("Helpers' model", "frontier", dialog());
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
    expect(
      await within(dialog()).findByText(
        "Someone changed this setting since you opened it. Reload to see the current value.",
      ),
    ).toBeVisible();
  });

  it("sets the check as one line, and refuses a longer one before sending it", async () => {
    const fake = fakeLimits({ limits: [...ROOM, ...TEAM_LIMITS] });
    show(fake.api);
    await screen.findByRole("row", { name: /Check before a team finishes/ });
    edit("Check before a team finishes");
    const command = within(dialog()).getByRole("textbox", { name: "Command" });
    fireEvent.change(command, { target: { value: "x".repeat(4097) } });
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
    expect(
      await within(dialog()).findByText(
        "Enter one line of at most 4 096 bytes.",
      ),
    ).toBeVisible();
    expect(fake.teamWrites).toEqual([]);
    fireEvent.change(command, { target: { value: "  cargo test  " } });
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
    expect(
      await screen.findByText(
        "Saved Check before a team finishes: cargo test.",
      ),
    ).toBeVisible();
    expect(fake.teamWrites).toEqual([
      { member_route: null, acceptance_check: "cargo test", version: 0 },
    ]);
    // Emptied, there is no check again.
    edit("Check before a team finishes");
    fireEvent.change(
      within(dialog()).getByRole("textbox", { name: "Command" }),
      { target: { value: " " } },
    );
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
    expect(
      await screen.findByText("Saved Check before a team finishes: None."),
    ).toBeVisible();
  });

  it("says when the helpers' settings can't be read, and reads them again", async () => {
    const fake = fakeLimits({ limits: [...ROOM, ...TEAM_LIMITS] });
    const read = fake.api.agentTeams.bind(fake.api);
    let failures = 1;
    fake.api.agentTeams = (signal) =>
      failures-- > 0 ? Promise.reject(new Error("offline")) : read(signal);
    show(fake.api);
    expect(
      await screen.findByText(
        "The helpers' model and check could not be loaded.",
      ),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(
      await screen.findByRole("row", { name: /Helpers' model/ }),
    ).toHaveTextContent("The lead task's model");
  });

  it("shows no helper section while agent teams are off", async () => {
    const fake = fakeLimits({ teams: null });
    let reads = 0;
    fake.api.agentTeams = () => {
      reads += 1;
      return Promise.reject(new Error("not asked"));
    };
    show(fake.api);
    await screen.findByRole("table", { name: "Limits" });
    expect(screen.queryByRole("heading", { name: "Helpers" })).toBeNull();
    expect(screen.queryByText(/helper/i)).toBeNull();
    // Off means no trace: the page never asks for the helpers' settings either.
    expect(reads).toBe(0);
  });
});

// WEB-U-248 (ADR-0039 §9-10, GW-U-146): the Desktop's two limits in their own section, in words like every limit,
// with the range in the edit dialog and the gateway's refusals in words.
describe("WEB-U-248 the Desktop's limits", () => {
  const desktop = () =>
    fakeLimits({ limits: [...ADMIN_LIMITS, ...DESKTOP_LIMITS] });

  it("lists them in their own section, in words, apart from the others", async () => {
    show(desktop().api);
    const section = await screen.findByRole("table", {
      name: "Desktop limits",
    });
    expect(screen.getByRole("heading", { name: "Desktop" })).toBeVisible();
    // The intro is short; each row says when a change applies (a UX review before 17.0).
    expect(
      screen.getByText(
        "For tasks on a Desktop environment. Each limit says when a change applies.",
      ),
    ).toBeVisible();
    const sessions = within(section).getByRole("row", {
      name: /Desktop environments at once/,
    });
    expect(sessions).toHaveTextContent("2 tasks");
    expect(sessions).toHaveTextContent("Applies at the next task start");
    const kept = within(section).getByRole("row", {
      name: /Keep action screenshots for/,
    });
    expect(kept).toHaveTextContent("30 days");
    expect(kept).toHaveTextContent("Applies at the next clean-up");
    // The page's other limits follow its own intro, with no line of their own.
    expect(
      within(screen.getByRole("table", { name: "Limits" })).queryByText(
        /^Applies /,
      ),
    ).toBeNull();
    expect(
      within(screen.getByRole("table", { name: "Limits" })).queryByText(
        /Desktop environments at once/,
      ),
    ).toBeNull();
  });

  it("states the range, and refuses more Desktop environments than tasks at once in words", async () => {
    const fake = desktop();
    show(fake.api);
    await screen.findByRole("table", { name: "Desktop limits" });
    edit("Keep action screenshots for");
    // The range is in the field's own unit, as for every limit: the field says days.
    expect(within(dialog()).getByLabelText("Value (days)")).toBeVisible();
    expect(within(dialog()).getByText("From 1 to 365.")).toBeVisible();
    expect(
      within(dialog()).getByText(
        "A change applies at the gateway's next removal of old screenshots.",
      ),
    ).toBeVisible();
    fireEvent.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    edit("Desktop environments at once");
    enter("9");
    save();
    // The gateway lists this limit's max as the slots, so the page refuses more itself, saying the rule and its number,
    // not only a range (WEB-E2E-067: the stack meets this, never the gateway's own refusal).
    expect(
      await within(dialog()).findByText(
        "Desktop environments at once can't be more than Tasks running at once, which is 8 now.",
      ),
    ).toBeVisible();
    expect(within(dialog()).queryByText(/Enter a whole number/)).toBeNull();
  });

  it("shows no Desktop section when the gateway lists no Desktop limit", async () => {
    show(fakeLimits().api);
    await screen.findByRole("table", { name: "Limits" });
    expect(screen.queryByRole("heading", { name: "Desktop" })).toBeNull();
  });
});

// WEB-U-280 (ADR-0039 §9-10): the computer-use caps per run sit in the Desktop section beside the Desktop's limits,
// counted in words, and each says when it applies: in a few words on its row, and whole in its edit dialog.
describe("WEB-U-280 the computer-use caps per run", () => {
  it("lists them under Desktop, in words, and says they apply to desktops that start after the change", async () => {
    show(fakeLimits({ limits: [...ADMIN_LIMITS, ...DESKTOP_LIMITS] }).api);
    const section = await screen.findByRole("table", {
      name: "Desktop limits",
    });
    expect(
      within(section).getByRole("row", {
        name: /Screenshots a run may send the model/,
      }),
      // WEB-U-372 (a live first-hour check before 17.0): a running desktop keeps the caps it linked with, so the row
      // says when it applies as it works: to desktops that start after the change, never "the next run".
    ).toHaveTextContent(
      /200 screenshots.*Applies to desktops that start after the changeEdit$/,
    );
    expect(
      within(section).getByRole("row", {
        name: /Desktop actions a run may take/,
      }),
    ).toHaveTextContent(
      /1.000 actions.*Applies to desktops that start after the changeEdit$/,
    );
    expect(
      within(screen.getByRole("table", { name: "Limits" })).queryByText(
        /a run may/,
      ),
    ).toBeNull();
    edit("Desktop actions a run may take");
    expect(within(dialog()).getByLabelText("Value (actions)")).toBeVisible();
    expect(within(dialog()).getByText("From 1 to 5000.")).toBeVisible();
    expect(
      within(dialog()).getByText(
        "A change applies to Desktop environments that start after it; a desktop already running keeps the cap it started with.",
      ),
    ).toBeVisible();
  });
});

// WEB-U-361 (a pass over the pages before 17.0): on a phone each limit is a card, and its value needs no "Value" label
// above it: the limit's name says what it is (the column header stays for screen readers and wide screens).
describe("WEB-U-361 a limit's card names no Value", () => {
  it("leaves the value cells unlabelled", async () => {
    show(fakeLimits().api);
    const table = await screen.findByRole("table", { name: "Limits" });
    const values = [...table.querySelectorAll("tbody tr")].map(
      (row) => row.children[1] as HTMLElement,
    );
    expect(values.length).toBeGreaterThan(0);
    for (const cell of values) expect(cell).not.toHaveAttribute("data-label");
    expect(
      screen.getAllByRole("columnheader").map((cell) => cell.textContent),
    ).toEqual(["Limit", "Value", "Actions"]);
  });
});
