import { readFileSync } from "node:fs";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { formatMessage } from "../i18n/message";
import type { Translate } from "../i18n/translate";
import type { AdminRoute, User } from "../protocol/generated/api";
import { LIMITS } from "../test/user";

import type { OverviewApi } from "./overview";
import { modelsLine, OverviewSettings } from "./OverviewSettings";

const replace = vi.fn();
vi.mock("next/navigation", () => ({ useRouter: () => ({ replace }) }));

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const checks = {
  backend_preflight: true,
  image_present: true,
  database: true,
  secret_store: true,
  runtime_revision: true,
};
const route = (name: string, state: AdminRoute["probe"]["state"]) =>
  ({ name, probe: { state } }) as AdminRoute;

function fake(overrides: Partial<OverviewApi> = {}): OverviewApi {
  return {
    readiness: () => Promise.resolve({ ready: true, checks }),
    routes: () =>
      Promise.resolve([
        route("deepseek", "verified"),
        route("local", "verified"),
        route("gemini", "never"),
      ]),
    people: () => Promise.resolve({ active: 12, invited: 2 }),
    internetAccess: () => Promise.resolve({ disabled: false, rules: 0 }),
    rotationDue: () => Promise.resolve(1),
    desktop: () => Promise.resolve(false),
    ...overrides,
  };
}
function show(api: OverviewApi, me: User = admin) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <OverviewSettings me={me} api={api} />
    </QueryClientProvider>,
  );
}

// WEB-U-077: an administrator lands on Settings and sees whether tasks can start, what is set up and what is missing,
// each with the action that fixes it (UI-GUIDELINES §7).
describe("WEB-U-077 settings overview", () => {
  it("says tasks can start, sums up models and people, and lists what's missing with its fix", async () => {
    show(fake());
    expect(
      await screen.findByText("All 5 checks pass. Tasks can start."),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "View status" })).toHaveAttribute(
      "href",
      "/settings/status",
    );
    expect(
      await screen.findByText("3 models: 1 not checked, 2 verified"),
    ).toBeInTheDocument();
    // Said once, in plain words, with the action while some are unchecked.
    expect(screen.getByRole("link", { name: "Check them" })).toHaveAttribute(
      "href",
      "/settings/models",
    );
    expect(
      await screen.findByText("12 people · 2 invitations waiting"),
    ).toBeInTheDocument();
    const attention = await screen.findByRole("region", {
      name: "Needs attention",
    });
    expect(
      within(attention)
        .getAllByRole("link")
        .map((link) => [link.textContent, link.getAttribute("href")]),
    ).toEqual([
      ["Allow sites", "/settings/internet-access"],
      ["Review keys", "/settings/keys/shared"],
    ]);
    // WEB-U-116: connectors are optional, so an install without any needs no attention for it.
    expect(attention).not.toHaveTextContent(/connector/i);
    expect(
      within(attention).getByText(
        "No internet access is allowed yet. Tasks can't install packages.",
      ),
    ).toBeInTheDocument();
    expect(
      within(attention).getByText("1 shared key is due for rotation."),
    ).toBeInTheDocument();
  });

  it("names what fails when tasks can't start, and leaves out what is already set up", async () => {
    show(
      fake({
        readiness: () =>
          Promise.resolve({
            ready: false,
            checks: { ...checks, database: false },
          }),
        internetAccess: () => Promise.resolve({ disabled: false, rules: 4 }),
        rotationDue: () => Promise.resolve(0),
        routes: () =>
          Promise.resolve([
            route("deepseek", "verified"),
            route("local", "verified"),
          ]),
      }),
    );
    expect(
      await screen.findByText(
        "1 check fails: Database. New tasks can't start.",
      ),
    ).toBeInTheDocument();
    await screen.findByText("12 people · 2 invitations waiting");
    expect(await screen.findByText("All 2 verified")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Check them" })).toBeNull();
    expect(
      screen.queryByRole("region", { name: "Needs attention" }),
    ).toBeNull();
  });

  // What the Models card says, most urgent first: a failed check (a warning, and the action to check), then one out
  // of date, then one not checked, then all verified. Several states at once are counted, most urgent first (GW-I-140).
  const models = async (
    states: AdminRoute["probe"]["state"][],
  ): Promise<HTMLElement> => {
    show(
      fake({
        routes: () =>
          Promise.resolve(states.map((state, n) => route(`r${n}`, state))),
      }),
    );
    const line = (await screen.findByText("Models")).closest(".overview-line");
    if (!(line instanceof HTMLElement)) throw new Error("no Models line");
    await within(line).findByRole("link");
    return line;
  };

  it.each([
    [["failed"], "1 check failed", "warn", "Check them"],
    [["failed", "failed"], "2 checks failed", "warn", "Check them"],
    [["stale"], "1 out of date", undefined, "Check them"],
    [["never", "never"], "2 not checked", undefined, "Check them"],
    [["verified"], "1 verified", "ok", "Manage"],
    [["verified", "verified"], "All 2 verified", "ok", "Manage"],
    [[], "No models yet", undefined, "Manage"],
  ] as const)("says %j as %s", async (states, copy, state, action) => {
    const line = await models([...states]);
    expect(within(line).getByText(copy)).toBeInTheDocument();
    expect(line.dataset.state).toBe(state);
    expect(within(line).getByRole("link", { name: action })).toHaveAttribute(
      "href",
      "/settings/models",
    );
  });

  it("counts several states at once, most urgent first, with its warning", async () => {
    const line = await models(["never", "failed", "verified", "stale"]);
    expect(
      within(line).getByText(
        "4 models: 1 check failed, 1 out of date, 1 not checked, 1 verified",
      ),
    ).toBeInTheDocument();
    expect(line.dataset.state).toBe("warn");
    expect(
      within(line).getByRole("link", { name: "Check them" }),
    ).toBeVisible();
  });

  it("never says a failed check is not checked yet", async () => {
    const line = await models(["never", "failed"]);
    expect(
      within(line).getByText("2 models: 1 check failed, 1 not checked"),
    ).toBeInTheDocument();
    expect(within(line).queryByText(/checked yet/)).toBeNull();
  });

  // Each language lists the states its own way: Chinese joins them with "、", never with nothing between them.
  it("lists several states as the reader's language lists things", () => {
    const zh = (
      JSON.parse(readFileSync("src/i18n/locales/zh-Hans.json", "utf8")) as {
        messages: Record<string, string>;
      }
    ).messages;
    const t = ((key: string, values?: Record<string, string | number>) =>
      formatMessage(zh[key] ?? "", values ?? {}, "zh-Hans")) as Translate;
    const routes = (["verified", "never", "failed"] as const).map((state, n) =>
      route(`r${n}`, state),
    );
    expect(modelsLine(t, "zh-Hans", routes).text).toBe(
      "3 个模型：1 个检查失败、1 个未检查、1 个已验证",
    );
  });

  it("keeps the rest when one part can't load", async () => {
    show(fake({ routes: () => Promise.reject(new Error("offline")) }));
    expect(
      await screen.findByText("The models could not be loaded."),
    ).toBeInTheDocument();
    expect(
      await screen.findByText("12 people · 2 invitations waiting"),
    ).toBeInTheDocument();
  });

  it("sends a member to their account", () => {
    show(fake(), { ...admin, role: "member" });
    expect(replace).toHaveBeenCalledWith("/settings/account");
  });
});

// A first-hour check before 17.0: the Desktop and agent teams are install-time choices, so the administrator's landing
// page names each when it's there, with where it's managed; when it isn't, nothing about it shows (WEB-U-318).
describe("WEB-U-318 Overview names c16's features when they're available", () => {
  it("names the Desktop and agent teams when the install has them", async () => {
    show(fake({ desktop: () => Promise.resolve(true) }), {
      ...admin,
      features: { agent_teams: true },
    });
    expect(
      await screen.findByText(
        "Tasks can use a desktop: choose a Desktop environment for a task.",
      ),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Environments" })).toHaveAttribute(
      "href",
      "/settings/environments",
    );
    expect(
      screen.getByText("On: a task leads helpers when you ask it to."),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Helper limits" })).toHaveAttribute(
      "href",
      "/settings/limits",
    );
  });

  it("says nothing about either when the install has neither", async () => {
    show(fake());
    await screen.findByText("All 5 checks pass. Tasks can start.");
    await screen.findByText("12 people · 2 invitations waiting");
    expect(screen.queryByText(/desktop/i)).toBeNull();
    expect(screen.queryByText(/helpers/i)).toBeNull();
  });
});

// WEB-U-360 (a pass over the pages before 17.0): a line that can't be read offers Try again, which reads it again, as
// the other settings pages' load failures do.
describe("WEB-U-360 Try again on the overview", () => {
  it("reads the status again", async () => {
    let calls = 0;
    show(
      fake({
        readiness: () =>
          ++calls === 1
            ? Promise.reject(new Error("offline"))
            : Promise.resolve({ ready: true, checks }),
      }),
    );
    expect(
      await screen.findByText("The status could not be loaded."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(
      await screen.findByText("All 5 checks pass. Tasks can start."),
    ).toBeInTheDocument();
    expect(calls).toBe(2);
  });
});
