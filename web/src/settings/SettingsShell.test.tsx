import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { User } from "../protocol/generated/api";
import { ShellContext } from "../shell/shell-context";
import { LIMITS } from "../test/user";

import { SettingsShell } from "./SettingsShell";

const push = vi.hoisted(() => vi.fn<(href: string) => void>());
const where = vi.hoisted(() => ({ path: "/settings/tokens" }));
vi.mock("next/navigation", () => ({
  usePathname: () => where.path,
  useRouter: () => ({ push }),
}));

function show(user: User, section: "tokens" | "keys" = "tokens") {
  const cache = new QueryClient();
  cache.setQueryData(["session"], user);
  render(
    <QueryClientProvider client={cache}>
      <SettingsShell section={section}>
        {(me) => <p>Signed in as {me.email}</p>}
      </SettingsShell>
    </QueryClientProvider>,
  );
}

const groups = () =>
  screen
    .getAllByRole("group")
    .filter((group) => group.closest("nav"))
    .map((group) => [
      group.getAttribute("aria-label"),
      within(group)
        .getAllByRole("link")
        .map((link) => [link.textContent, link.getAttribute("href")]),
    ]);

// UI-GUIDELINES §6: settings are a column with its sections on the left. Since WEB-U-075 (plan §1 in UI-GUIDELINES §7)
// they come in three groups: Your account for everyone; Administration and Operations for administrators only.
describe("WEB-U-023/075 settings", () => {
  it("shows a member only Your account, and marks the current section", () => {
    show({
      id: "0195d8c0-0000-7000-8000-000000000002",
      email: "member@example.invalid",
      role: "member",
      internet: "none",
      limits: LIMITS,
      features: { agent_teams: false },
    });
    expect(
      screen.getByRole("heading", { level: 1, name: "Settings" }),
    ).toBeInTheDocument();
    // WEB-U-038/044: Git and connector credentials are one page now, Keys; WEB-U-049: task profiles.
    expect(groups()).toEqual([
      [
        "Your account",
        [
          ["Account", "/settings/account"],
          ["API tokens", "/settings/tokens"],
          ["Keys", "/settings/keys"],
          ["Agent profiles", "/settings/agent-profiles"],
        ],
      ],
    ]);
    const sections = screen.getByRole("navigation", {
      name: "Settings sections",
    });
    expect(
      within(sections).getByRole("link", { name: "API tokens" }),
    ).toHaveAttribute("aria-current", "page");
    expect(
      screen.getByText("Signed in as member@example.invalid"),
    ).toBeInTheDocument();
    expect(document.title).toBe("API tokens · TensorQuay Agent");
  });

  // WEB-U-033/034/035/039/040/041/043/047: the administration sections, for administrators only.
  it("adds Administration and Operations for administrators, in plain words", () => {
    show({
      id: "0195d8c0-0000-7000-8000-000000000001",
      email: "admin@example.invalid",
      role: "admin",
      internet: "none",
      limits: LIMITS,
      features: { agent_teams: false },
    });
    expect(groups()).toEqual([
      [
        "Your account",
        [
          ["Account", "/settings/account"],
          ["API tokens", "/settings/tokens"],
          ["Keys", "/settings/keys"],
          ["Agent profiles", "/settings/agent-profiles"],
        ],
      ],
      [
        "Administration",
        [
          ["Overview", "/settings"],
          ["People", "/settings/people"],
          ["Models", "/settings/models"],
          ["Environments", "/settings/environments"],
          ["Connectors", "/settings/connectors"],
          ["Decision engines", "/settings/decision-engines"],
          ["Internet access", "/settings/internet-access"],
          ["Limits", "/settings/limits"],
        ],
      ],
      [
        "Operations",
        [
          ["Usage", "/settings/usage"],
          ["Status", "/settings/status"],
        ],
      ],
    ]);
  });

  // The owner's ask: a 16 px line icon before each section, as in the main sidebar; decorative, so names don't change.
  it("puts one hidden line icon before each section's name", () => {
    show({
      id: "0195d8c0-0000-7000-8000-000000000001",
      email: "admin@example.invalid",
      role: "admin",
      internet: "none",
      limits: LIMITS,
      features: { agent_teams: false },
    });
    const sections = screen.getByRole("navigation", {
      name: "Settings sections",
    });
    const icons = Object.fromEntries(
      within(sections)
        .getAllByRole("link")
        .map((link) => {
          const svgs = link.querySelectorAll("svg");
          expect(svgs).toHaveLength(1);
          expect(svgs[0]).toHaveAttribute("aria-hidden", "true");
          expect(svgs[0]).toHaveAttribute("width", "16");
          // The icon comes first, the name after it.
          expect(link.firstElementChild?.tagName.toLowerCase()).toBe("svg");
          const kind = [...(svgs[0]?.classList ?? [])].find(
            (name) => name !== "lucide" && name.startsWith("lucide-"),
          );
          return [link.textContent, kind];
        }),
    );
    expect(icons).toEqual({
      Account: "lucide-user",
      "API tokens": "lucide-terminal",
      Keys: "lucide-key-round",
      "Agent profiles": "lucide-bot",
      Overview: "lucide-layout-dashboard",
      People: "lucide-users",
      Models: "lucide-sparkles",
      Environments: "lucide-box",
      Connectors: "lucide-plug",
      "Decision engines": "lucide-route",
      "Internet access": "lucide-globe",
      Limits: "lucide-gauge",
      Usage: "lucide-chart-column",
      Status: "lucide-activity",
    });
    expect(
      within(sections).getByRole("link", { name: "API tokens" }),
    ).toHaveAccessibleName("API tokens");
  });

  // WEB-U-134: the standard picker's grouped menu, full width, never a native select.
  it("offers the same sections as one grouped picker for small screens", () => {
    show({
      id: "0195d8c0-0000-7000-8000-000000000001",
      email: "admin@example.invalid",
      role: "admin",
      internet: "none",
      limits: LIMITS,
      features: { agent_teams: false },
    });
    const picker = screen.getByRole("button", { name: "Section: API tokens" });
    expect(picker).toHaveClass("picker-field");
    fireEvent.keyDown(picker, { key: "ArrowDown" });
    const menu = screen.getByRole("menu", { name: "Section" });
    expect(within(menu).getAllByRole("group")).toHaveLength(3);
    for (const name of ["Your account", "Administration", "Operations"])
      expect(within(menu).getByRole("group", { name })).toBeInTheDocument();
    expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(14);
    fireEvent.click(
      within(menu).getByRole("menuitemradio", { name: "Models" }),
    );
    expect(push).toHaveBeenCalledWith("/settings/models");
  });
});

// WEB-U-362 (the 16f page pass, E17): below 768 px the sidebar is a drawer, so Settings starts with Open navigation,
// as every page in the shell does; outside the shell there is nothing to open.
// 17.3: every Settings section opens with its name as the heading (20/1.25), then its one muted line and its primary
// action; a page inside a section (adding or editing one item) keeps its own title under its way back.
describe("WEB-U-442 a section opens with its name", () => {
  const admin: User = {
    id: "0195d8c0-0000-7000-8000-000000000002",
    email: "member@example.invalid",
    role: "admin",
    internet: "none",
    limits: LIMITS,
    features: { agent_teams: false },
  };

  it("heads the section's own page with its name, before what the section shows", () => {
    where.path = "/settings/tokens";
    show(admin);
    const heading = screen.getByRole("heading", {
      level: 2,
      name: "API tokens",
    });
    expect(heading).toHaveClass("type-heading");
    expect(heading.nextElementSibling).toHaveTextContent(
      "Signed in as member@example.invalid",
    );
  });

  it("heads a view of the section too: the shared keys are the Keys section", () => {
    where.path = "/settings/keys/shared";
    show(admin, "keys");
    expect(
      screen.getByRole("heading", { level: 2, name: "Keys" }),
    ).toBeInTheDocument();
    where.path = "/settings/tokens";
  });

  it("leaves a page inside the section its own title", () => {
    where.path = "/settings/tokens/new";
    show(admin);
    expect(
      screen.queryByRole("heading", { level: 2, name: "API tokens" }),
    ).toBeNull();
    where.path = "/settings/tokens";
  });
});

describe("WEB-U-362 Open navigation in Settings", () => {
  const user: User = {
    id: "0195d8c0-0000-7000-8000-000000000002",
    email: "member@example.invalid",
    role: "member",
    internet: "none",
    limits: LIMITS,
    features: { agent_teams: false },
  };

  it("opens the shell's navigation", () => {
    const openNavigation = vi.fn<() => void>();
    const cache = new QueryClient();
    cache.setQueryData(["session"], user);
    render(
      <ShellContext.Provider value={{ openNavigation }}>
        <QueryClientProvider client={cache}>
          <SettingsShell section="account">
            {() => <p>Account</p>}
          </SettingsShell>
        </QueryClientProvider>
      </ShellContext.Provider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Open navigation" }));
    expect(openNavigation).toHaveBeenCalledTimes(1);
  });

  it("shows none outside the shell", () => {
    show(user);
    expect(
      screen.queryByRole("button", { name: "Open navigation" }),
    ).toBeNull();
  });
});
