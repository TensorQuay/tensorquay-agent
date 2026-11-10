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
import { engine, fakeEngines } from "../test/decision-engines";
import { choose, picker } from "../test/picker";
import { LIMITS } from "../test/user";

import type { EnginesApi } from "./admin/decision-engines";
import { DecisionEnginesSettings } from "./DecisionEnginesSettings";

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
function show(api: EnginesApi, me: User = admin) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <DecisionEnginesSettings me={me} api={api} />
    </QueryClientProvider>,
  );
}
const row = (name: RegExp) => screen.getByRole("row", { name });
/** A short line's two forms: what shows (hidden from screen readers), and the exact words screen readers get instead. */
const forms = (line: HTMLElement) => ({
  shown: line.querySelector('[aria-hidden="true"]')?.textContent,
  read: line.querySelector(".visually-hidden")?.textContent,
});
/** A count written with the narrow no-break space between its thousands, as the app writes it (§10). */
const grouped = (words: string) => words.replace(/(\d) (?=\d)/g, "$1\u202f");
const field = (name: string) => screen.getByRole("textbox", { name });

/** Opens the add dialog, takes its first step (the kind), and waits for the form. */
async function openAdd(kind = "Your own engine") {
  fireEvent.click(
    await screen.findByRole("button", { name: "Add decision engine" }),
  );
  const dialog = screen.getByRole("dialog", { name: "Add a decision engine" });
  fireEvent.click(within(dialog).getByRole("radio", { name: kind }));
  fireEvent.click(within(dialog).getByRole("button", { name: "Continue" }));
  // The secrets load with the form; each is offered by its name (the standard picker, WEB-U-134).
  fireEvent.click(picker("Key", dialog));
  await screen.findByRole("menuitemradio", { name: /^engine-key/ });
  fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
  return dialog;
}
function fillNew(dialog: HTMLElement, key = "secrets/engine-key") {
  fireEvent.change(field("Display name"), {
    target: { value: "Policy engine" },
  });
  fireEvent.change(field("Engine address"), {
    target: { value: "https://policy.example" },
  });
  // A key by its name; none chosen leaves the picker on "Choose a key".
  if (key) choose("Key", key.replace(/^secrets\//, ""), dialog);
}
const save = (dialog: HTMLElement) =>
  fireEvent.click(within(dialog).getByRole("button", { name: "Save engine" }));
async function menu(name: string) {
  fireEvent.click(
    await screen.findByRole("button", { name: `More actions for ${name}` }),
  );
  return screen.getByRole("menu", { name: `More actions for ${name}` });
}

// TEST: WEB-U-074
describe("WEB-U-074 decision engines", () => {
  it("lists each engine by its display name, with its host, what it sees and its status, never a secret's ref", async () => {
    show(
      fakeEngines({
        engines: [
          engine("router", {
            display_name: "Policy engine",
            exposure: "calls",
            key_ref: "secrets/gone",
            key_missing: true,
            seed: "current",
          }),
          engine("reviewer", { enabled: false, exposure: "full" }),
          engine("plain"),
        ],
      }).api,
    );
    await screen.findByRole("table", { name: "Decision engines" });
    const router = row(/Policy engine/);
    for (const text of ["Names and arguments", "Key missing"])
      expect(within(router).getByText(text)).toBeVisible();
    // Its host and, for a seeded one, its source: one muted line under the name (17.3).
    expect(
      cellNote(router, "router.example · From the configuration file"),
    ).toBeVisible();
    const reviewer = row(/reviewer/);
    for (const text of ["reviewer.example", "Full conversation", "Off"])
      expect(within(reviewer).getByText(text)).toBeVisible();
    expect(within(row(/plain/)).getByText("On")).toBeVisible();
    expect(screen.queryByText(/secrets\//)).toBeNull();
    expect(screen.queryByText(/Added here/)).toBeNull();
    // One visible action per row; the rest in its "…" menu.
    expect(
      within(router)
        .getAllByRole("button")
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual([
      "Edit decision engine Policy engine",
      "More actions for Policy engine",
    ]);
  });

  it("shows each budget's use over its current count on the engine's row", async () => {
    show(
      fakeEngines({
        engines: [
          engine("monthly", {
            budget: { calls: 3000, period: "month" },
            budget_used: { calls: 1204, tokens: 0 },
            budget_since: "2026-09-01T00:00:00Z",
          }),
          engine("typed", {
            protocol: "typed_questions_v1",
            model: "pinned-model",
            budget: { calls: 500, tokens: 30000000, period: "total" },
            budget_used: { calls: 12, tokens: 4200 },
            budget_since: "2026-09-29T12:00:00Z",
          }),
          engine("keyless", {
            key_ref: "secrets/gone",
            key_missing: true,
            budget: { calls: 100, period: "month" },
            budget_used: { calls: 7, tokens: 0 },
            budget_since: "2026-09-01T00:00:00Z",
          }),
          engine("plain"),
        ],
      }).api,
    );
    await screen.findByRole("table", { name: "Decision engines" });
    expect(
      within(row(/monthly/)).getByText(/^1\s204 of 3\s000 calls this month$/),
    ).toBeVisible();
    // One muted line for both limits (17.3); no page says the use, so the line's tooltip holds it whole, its figures
    // exact (WEB-U-461).
    const typed = row(/typed/);
    const typedExact =
      "12 of 500 calls since Sep 29, 2026 · 4\u202f200 of 30\u202f000\u202f000 tokens since Sep 29, 2026";
    expect(forms(cellNote(typed, "12 of 500 calls", typedExact))).toEqual({
      shown:
        "12 of 500 calls since Sep 29, 2026 · 4.2K of 30M tokens since Sep 29, 2026",
      read: typedExact,
    });
    // A missing key's link leads the line, and the use still follows it.
    const keyless = row(/keyless/);
    expect(
      within(keyless).getByRole("link", { name: "Add it under Keys › Shared" }),
    ).toHaveAttribute("href", "/settings/keys/shared");
    expect(cellNote(keyless, "Add it under Keys › Shared")).toHaveTextContent(
      "Add it under Keys › Shared · 7 of 100 calls this month",
    );
    expect(within(row(/plain/)).queryByText(/ of /)).toBeNull();
  });

  // WEB-U-461 (17.4 fix): the short figures never misstate where a budget stands. Under its limit the use
  // rounds down and the limit up; at or over it, the use rounds up and the limit down (both are multiples of the step
  // shown, so under never reads as equal and at or over never reads as under). The line shows them short; its tooltip
  // and screen readers get them exactly, and a missing key's link is read as itself.
  it("never shows a budget as more or less spent than it is, and gives screen readers the exact figures", async () => {
    const use = (used: number, limit: number) => ({
      budget: { tokens: limit, period: "month" as const },
      budget_used: { calls: 0, tokens: used },
      budget_since: "2026-09-01T00:00:00Z",
    });
    show(
      fakeEngines({
        engines: [
          engine("spent", use(29980000, 29960000)),
          engine("headroom", use(29930000, 29940000)),
          engine("round-under", use(29960000, 30000000)),
          engine("round-over", use(30040000, 30000000)),
          engine("keyless", {
            ...use(29960000, 30000000),
            key_ref: "secrets/gone",
            key_missing: true,
          }),
        ],
      }).api,
    );
    await screen.findByRole("table", { name: "Decision engines" });
    for (const [name, shown, exact] of [
      ["spent", "30M of 29.9M", "29 980 000 of 29 960 000"],
      ["headroom", "29.9M of 30M", "29 930 000 of 29 940 000"],
      ["round-under", "29.9M of 30M", "29 960 000 of 30 000 000"],
      ["round-over", "30.1M of 30M", "30 040 000 of 30 000 000"],
    ] as const) {
      const words = `${grouped(exact)} tokens this month`;
      const line = cellNote(row(new RegExp(name)), "tokens", words);
      expect(forms(line)).toEqual({
        shown: `${shown} tokens this month`,
        read: words,
      });
      expect(row(new RegExp(name))).toHaveAccessibleName(new RegExp(words));
    }
    const words = `${grouped("29 960 000 of 30 000 000")} tokens this month`;
    const keyless = cellNote(
      row(/keyless/),
      "Add it under Keys › Shared",
      `Add it under Keys › Shared · ${words}`,
    );
    expect(forms(keyless)).toEqual({
      shown: " · 29.9M of 30M tokens this month",
      read: ` · ${words}`,
    });
    expect(
      within(keyless).getByRole("link", { name: "Add it under Keys › Shared" }),
    ).toHaveAttribute("href", "/settings/keys/shared");
  });

  it("shows members nothing, and says what an empty list means", async () => {
    show(fakeEngines().api);
    expect(
      await screen.findByText(
        "No decision engines yet: the agent decides on its own.",
      ),
    ).toBeVisible();
    show(fakeEngines().api, { ...admin, role: "member" });
    expect(
      screen.getByText("Only administrators can manage decision engines."),
    ).toBeVisible();
  });

  it("asks for a display name, the address, the key and what it sees; derives the identifier under More options", async () => {
    const fake = fakeEngines();
    show(fake.api);
    const dialog = await openAdd();
    expect(
      within(dialog).getByRole("radio", { name: "Tool names only" }),
    ).toBeChecked();
    const more = within(dialog).getByRole("button", { name: "More options" });
    expect(more).toHaveAttribute("aria-expanded", "false");
    fillNew(dialog);
    fireEvent.click(more);
    expect(field("Identifier")).toHaveValue("policy-engine");
    save(dialog);
    expect(
      await screen.findByText("Saved decision engine Policy engine."),
    ).toBeVisible();
    expect(fake.writes[0]).toMatchObject({
      name: "policy-engine",
      fields: {
        display_name: "Policy engine",
        url: "https://policy.example",
        auth: { scheme: "bearer" },
        key_ref: "secrets/engine-key",
        exposure: "none",
        timeout_ms: 2000,
      },
    });
  });

  it("never saves an engine without a key choice, and saves one that needs none when told so", async () => {
    const fake = fakeEngines();
    show(fake.api);
    const dialog = await openAdd();
    fillNew(dialog, "");
    save(dialog);
    expect(
      await within(dialog).findByText(
        "Choose the engine's key, or None if it needs none.",
      ),
    ).toBeVisible();
    expect(fake.writes).toHaveLength(0);
    choose("Key", "None", dialog);
    // The error goes as soon as its field changes.
    expect(
      within(dialog).queryByText(
        "Choose the engine's key, or None if it needs none.",
      ),
    ).toBeNull();
    save(dialog);
    await waitFor(() => expect(fake.writes).toHaveLength(1));
    expect(fake.writes[0]?.fields.auth).toEqual({ scheme: "none" });
    expect(fake.writes[0]?.fields.key_ref).toBeUndefined();
  });

  it("starts with the kind, in plain words, and a multiple-choice service asks for its model, threshold and descriptions", async () => {
    const fake = fakeEngines();
    show(fake.api);
    fireEvent.click(
      await screen.findByRole("button", { name: "Add decision engine" }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "Add a decision engine",
    });
    // The first step is the kind alone: nothing else to fill yet.
    expect(
      within(dialog).getByRole("group", { name: "What kind of engine is it?" }),
    ).toBeVisible();
    expect(
      within(dialog).getByRole("radio", { name: "Your own engine" }),
    ).toBeChecked();
    expect(within(dialog).queryByRole("textbox")).toBeNull();
    expect(within(dialog).queryByText(/typed-question/i)).toBeNull();
    fireEvent.click(
      within(dialog).getByRole("radio", { name: "A multiple-choice service" }),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Continue" }));
    fireEvent.click(picker("Key", dialog));
    await screen.findByRole("menuitemradio", { name: /^engine-key/ });
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(within(dialog).getByText("A multiple-choice service")).toBeVisible();
    fillNew(dialog);
    save(dialog);
    expect(
      await within(dialog).findByText("Enter the model the service expects."),
    ).toBeVisible();
    expect(fake.writes).toHaveLength(0);
    fireEvent.change(field("Model"), { target: { value: "pinned-model" } });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "More options" }),
    );
    expect(
      within(dialog).getByText(/^Up to 10 seconds; 2 by default\./),
    ).toBeVisible();
    fireEvent.change(field("Confidence needed to switch routes"), {
      target: { value: "0.75" },
    });
    // One optional field per model route the installation has.
    const routes = within(dialog).getByRole("group", {
      name: "Route descriptions",
    });
    expect(
      within(routes)
        .getAllByRole("textbox")
        .map((input) => (input as HTMLInputElement).labels?.[0]?.textContent),
    ).toEqual(["local", "cheap"]);
    fireEvent.change(within(routes).getByRole("textbox", { name: "cheap" }), {
      target: { value: "routine steps" },
    });
    fireEvent.change(within(routes).getByRole("textbox", { name: "local" }), {
      target: { value: "hard steps" },
    });
    save(dialog);
    await waitFor(() => expect(fake.writes).toHaveLength(1));
    expect(fake.writes[0]?.fields).toMatchObject({
      protocol: "typed_questions_v1",
      model: "pinned-model",
      min_confidence: 0.75,
      route_hints: { cheap: "routine steps", local: "hard steps" },
    });
    expect(fake.writes[0]?.fields.budget).toBeUndefined();
  });

  it("keeps a description for a route that is gone, marked, and refuses one too long on its own field", async () => {
    const fake = fakeEngines({
      engines: [
        engine("router", {
          protocol: "typed_questions_v1",
          model: "pinned-model",
          route_hints: { gone: "an old route", cheap: "routine steps" },
        }),
      ],
    });
    show(fake.api);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Edit decision engine router",
      }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "Edit decision engine router",
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "More options" }),
    );
    const routes = await within(dialog).findByRole("group", {
      name: "Route descriptions",
    });
    // The fields follow the installation's routes once they have loaded.
    expect(
      await within(routes).findByRole("textbox", { name: "cheap" }),
    ).toHaveValue("routine steps");
    expect(
      within(routes).getByRole("textbox", {
        name: "gone (no longer a model route)",
      }),
    ).toHaveValue("an old route");
    const local = within(routes).getByRole("textbox", { name: "local" });
    fireEvent.change(local, { target: { value: "x".repeat(201) } });
    save(dialog);
    expect(
      await within(dialog).findByText(
        "Keep this description to one line of up to 200 characters.",
      ),
    ).toBeVisible();
    expect(local).toHaveAttribute("aria-invalid", "true");
    expect(fake.writes).toHaveLength(0);
    // Emptying a field drops that route's description; the others are kept as they are.
    fireEvent.change(local, { target: { value: "" } });
    fireEvent.change(within(routes).getByRole("textbox", { name: "cheap" }), {
      target: { value: "" },
    });
    save(dialog);
    await waitFor(() => expect(fake.writes).toHaveLength(1));
    expect(fake.writes[0]?.fields.route_hints).toEqual({
      gone: "an old route",
    });
  });

  it("goes back to the kind with Change, keeping what was typed", async () => {
    show(fakeEngines().api);
    const dialog = await openAdd();
    fillNew(dialog);
    fireEvent.click(within(dialog).getByRole("button", { name: "Change" }));
    fireEvent.click(
      within(dialog).getByRole("radio", { name: "A multiple-choice service" }),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Continue" }));
    expect(field("Display name")).toHaveValue("Policy engine");
    expect(field("Model")).toBeVisible();
  });

  it("shows the budget without More options, marks the wrong limit, groups big numbers and saves each month", async () => {
    const fake = fakeEngines();
    show(fake.api);
    const dialog = await openAdd("A multiple-choice service");
    fillNew(dialog);
    fireEvent.change(field("Model"), { target: { value: "pinned-model" } });
    expect(within(dialog).getByText("No budget")).toBeVisible();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Set a budget" }),
    );
    expect(
      within(dialog).getByText(/Counts from when this budget was saved/),
    ).toBeVisible();
    fireEvent.change(field("Calls"), { target: { value: "0" } });
    fireEvent.change(field("Tokens"), { target: { value: "30000000" } });
    save(dialog);
    expect(
      await within(dialog).findByText(
        "Calls must be 1 or more, or left empty.",
      ),
    ).toBeVisible();
    expect(field("Calls")).toHaveAttribute("aria-invalid", "true");
    expect(field("Tokens")).not.toHaveAttribute("aria-invalid", "true");
    expect(fake.writes).toHaveLength(0);
    // A field keeps exactly what was typed, focused or not, so selecting and typing replaces it; the summary under
    // the row reads the numbers grouped.
    expect(field("Tokens")).toHaveValue("30000000");
    fireEvent.focus(field("Tokens"));
    fireEvent.blur(field("Tokens"));
    expect(field("Tokens")).toHaveValue("30000000");
    fireEvent.change(field("Calls"), { target: { value: "3000" } });
    expect(
      within(dialog).getByText(
        /^Up to 3\s000 calls and 30M tokens in total\.$/,
      ),
    ).toBeVisible();
    fireEvent.click(picker("Per", dialog));
    expect(
      screen.getAllByRole("menuitemradio").map((option) => option.textContent),
    ).toEqual(["In total", "Each month (UTC)"]);
    fireEvent.click(
      screen.getByRole("menuitemradio", { name: /^Each month \(UTC\)/ }),
    );
    expect(
      within(dialog).getByText(
        /^Up to 3\s000 calls and 30M tokens each month \(UTC\)\.$/,
      ),
    ).toBeVisible();
    expect(
      within(dialog).queryByText(/Counts from when this budget was saved/),
    ).toBeNull();
    save(dialog);
    await waitFor(() => expect(fake.writes).toHaveLength(1));
    expect(fake.writes[0]?.fields).toMatchObject({
      budget: { calls: 3000, tokens: 30000000, period: "month" },
    });
  });

  it("gives your own engine a calls-only budget, and Remove the budget clears it", async () => {
    const fake = fakeEngines();
    show(fake.api);
    const dialog = await openAdd();
    fillNew(dialog);
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Set a budget" }),
    );
    expect(
      within(dialog).queryByRole("textbox", { name: "Tokens" }),
    ).toBeNull();
    fireEvent.change(field("Calls"), { target: { value: "500" } });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Remove the budget" }),
    );
    expect(within(dialog).getByText("No budget")).toBeVisible();
    save(dialog);
    await waitFor(() => expect(fake.writes).toHaveLength(1));
    expect(fake.writes[0]?.fields).toMatchObject({ protocol: "decision_v1" });
    expect(fake.writes[0]?.fields.budget).toBeUndefined();
  });

  it("clears each error as its field changes", async () => {
    show(fakeEngines().api);
    const dialog = await openAdd();
    save(dialog);
    for (const text of [
      "Give the engine a name people will recognise.",
      "Use the engine's https:// address, without a name, password or fragment.",
    ])
      expect(await within(dialog).findByText(text)).toBeVisible();
    fireEvent.change(field("Engine address"), {
      target: { value: "https://policy.example" },
    });
    expect(
      within(dialog).queryByText(
        "Use the engine's https:// address, without a name, password or fragment.",
      ),
    ).toBeNull();
    expect(
      within(dialog).getByText("Give the engine a name people will recognise."),
    ).toBeVisible();
  });

  it("asks before an engine sees more, in plain words, and saves only once confirmed", async () => {
    const fake = fakeEngines({
      engines: [engine("router", { display_name: "Policy engine" })],
    });
    show(fake.api);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Edit decision engine Policy engine",
      }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "Edit decision engine Policy engine",
    });
    fireEvent.click(
      within(dialog).getByRole("radio", { name: "The full conversation" }),
    );
    save(dialog);
    const confirm = await screen.findByRole("dialog", {
      name: "Let Policy engine see more?",
    });
    expect(
      within(confirm).getByText(/sees everything the model sees/),
    ).toBeVisible();
    expect(fake.writes).toHaveLength(0);
    fireEvent.click(
      within(confirm).getByRole("button", { name: "Let it see more" }),
    );
    await waitFor(() => expect(fake.writes).toHaveLength(1));
    expect(fake.writes[0]).toMatchObject({
      fields: { exposure: "full" },
      version: 1,
    });
  });

  it("narrows without asking", async () => {
    const fake = fakeEngines({
      engines: [engine("router", { exposure: "full" })],
    });
    show(fake.api);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Edit decision engine router",
      }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "Edit decision engine router",
    });
    fireEvent.click(
      within(dialog).getByRole("radio", { name: "Tool names only" }),
    );
    save(dialog);
    expect(
      await screen.findByText("Saved decision engine router."),
    ).toBeVisible();
    expect(screen.queryByRole("dialog", { name: /see more/ })).toBeNull();
    expect(fake.writes[0]).toMatchObject({ fields: { exposure: "none" } });
  });

  it("turns an engine off from its menu after saying what that does, and on again at once", async () => {
    const fake = fakeEngines({
      engines: [engine("router", { display_name: "Policy engine" })],
    });
    show(fake.api);
    fireEvent.click(
      within(await menu("Policy engine")).getByRole("menuitem", {
        name: "Turn off",
      }),
    );
    const confirm = screen.getByRole("dialog", {
      name: "Turn off Policy engine?",
    });
    expect(
      within(confirm).getByText(
        /tasks whose profile uses it are refused, and told why/,
      ),
    ).toBeVisible();
    fireEvent.click(within(confirm).getByRole("button", { name: "Turn off" }));
    await waitFor(() =>
      expect(fake.writes.at(-1)).toMatchObject({
        name: "router",
        fields: { enabled: false },
        version: 1,
      }),
    );
    expect(await screen.findByText("Turned off Policy engine.")).toBeVisible();
    fireEvent.click(
      within(await menu("Policy engine")).getByRole("menuitem", {
        name: "Turn on",
      }),
    );
    await waitFor(() =>
      expect(fake.writes.at(-1)).toMatchObject({
        fields: { enabled: true },
        version: 2,
      }),
    );
  });

  it("offers to allow a secret the engine may not use yet, by its name", async () => {
    const fake = fakeEngines({ bindings: { "secrets/engine-key": [] } });
    show(fake.api);
    const dialog = await openAdd();
    fillNew(dialog);
    save(dialog);
    expect(
      await within(dialog).findByText(
        "engine-key isn't allowed for this engine. Allow it first.",
      ),
    ).toBeVisible();
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Allow it for Policy engine",
      }),
    );
    await waitFor(() =>
      expect(fake.allowCalls).toEqual([
        { ref: "secrets/engine-key", name: "policy-engine" },
      ]),
    );
  });

  it("refuses to delete an engine in use and says to turn it off instead", async () => {
    const fake = fakeEngines({
      engines: [engine("router")],
      inUse: { router: { conversations: 2, profiles: ["reviewers"] } },
    });
    show(fake.api);
    fireEvent.click(
      within(await menu("router")).getByRole("menuitem", { name: "Delete" }),
    );
    const confirm = screen.getByRole("dialog", {
      name: "Delete decision engine router?",
    });
    fireEvent.click(
      within(confirm).getByRole("button", { name: "Delete engine" }),
    );
    expect(
      await within(confirm).findByText(
        "1 agent profile and 2 open tasks use this engine. Turn it off instead, or change those agent profiles first.",
      ),
    ).toBeVisible();
    expect(row(/router/)).toBeVisible();
  });
});
