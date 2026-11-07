import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { User } from "../protocol/generated/api";
import { chosen, picker } from "../test/picker";
import { showProfiles } from "../test/profile-app";
import { fakeProfiles, profile } from "../test/profiles";
import { LIMITS } from "../test/user";

import type { ProfilesApi } from "./profiles";
import type { ChoicesApi } from "./ProfilesSettings";

/** Replaced by the harness once it renders (showProfiles). */
const router = vi.hoisted(() => ({ push: vi.fn<(href: string) => void>() }));
vi.mock("next/navigation", () => ({ useRouter: () => router }));
vi.mock("next/link", async () => {
  const { createElement } = await import("react");
  return {
    default: ({ href, children, ...rest }: Record<string, unknown>) =>
      createElement("a", { href, ...rest }, children as never),
  };
});

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const member: User = { ...admin, role: "member" };
const choices: ChoicesApi = {
  models: () => Promise.resolve({ items: [] }),
  templates: () => Promise.resolve({ items: [] }),
  connectors: () => Promise.resolve({ items: [] }),
  engines: () =>
    Promise.resolve([
      {
        name: "router",
        label: "Policy engine",
        exposure: "full",
        enabled: true,
        keyMissing: false,
      },
      {
        name: "old",
        label: "Old engine",
        exposure: "none",
        enabled: false,
        keyMissing: false,
      },
    ]),
};
function show(api: ProfilesApi, me: User) {
  showProfiles(api, choices, me, router);
}
/** The profile's page (WEB-U-104): its form, from the title down to Save and Cancel. */
const page = () => {
  const form = screen
    .getByRole("group", { name: "Save or cancel" })
    .closest("form");
  if (!form) throw new Error("The page is a form");
  return form;
};
/** Follows a link on the list to a profile's page, and waits for it. */
async function go(link: string) {
  fireEvent.click(await screen.findByRole("link", { name: link }));
  await screen.findByRole("group", { name: "Save or cancel" });
}

// TEST: WEB-U-074
// A profile names the decision engine its tasks consult (ADR-0013 point 6): administrators choose from the engines;
// members, who can't list them, keep what is set.
describe("WEB-U-074 a profile's decision engine", () => {
  it("lets an administrator choose an engine, none by default, and saves it", async () => {
    const shared = profile("ops", { display_name: "Ops", shared: true });
    const fake = fakeProfiles({ profiles: [shared], admin: true });
    show(fake.api, admin);
    await go("Edit agent profile Ops");
    await waitFor(() =>
      expect(chosen("Decision engine", page())).toBe(
        "None: the agent decides on its own",
      ),
    );
    // Each engine says what it would see, and one that is off says so.
    fireEvent.click(picker("Decision engine", page()));
    expect(
      await screen.findByRole("menuitemradio", {
        name: "Policy engine · sees the full conversation",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("menuitemradio", { name: "Old engine · off" }),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("menuitemradio", {
        name: "Policy engine · sees the full conversation",
      }),
    );
    fireEvent.click(within(page()).getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Saved agent profile Ops.")).toBeVisible();
    expect(fake.writes.at(-1)?.fields.decision_engine).toBe("router");
  });

  it("shows members no engine choice and keeps the engine a profile names", async () => {
    const own = profile("mine", {
      display_name: "Mine",
      decision_engine: "router",
    });
    const fake = fakeProfiles({ profiles: [own] });
    show(fake.api, member);
    await go("Edit agent profile Mine");
    expect(
      within(page()).queryByRole("button", { name: /^Decision engine: / }),
    ).toBeNull();
    fireEvent.click(within(page()).getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Saved agent profile Mine.")).toBeVisible();
    expect(fake.writes.at(-1)?.fields.decision_engine).toBe("router");
  });
});
