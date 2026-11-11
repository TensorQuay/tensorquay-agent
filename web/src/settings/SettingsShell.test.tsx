import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { type ReactNode, useEffect } from "react";
import { describe, expect, it, vi } from "vitest";

import { api } from "../api/client";
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

  // Decided before 17.0: a 16 px line icon before each section, as in the main sidebar; decorative, so no name changes.
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

// WEB-U-362 (a pass over the pages before 17.0): below 768 px the sidebar is a drawer, so Settings starts with Open
// navigation, as every page in the shell does; outside the shell there is nothing to open.
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

// WEB-U-460: a menu taller than the window scrolls on its own; the current section is never left out of its view, and
// only the menu scrolls to show it (17.4 fix): the page stays where it is.
describe("WEB-U-460 the Settings menu stays in view", () => {
  const member: User = {
    id: "0195d8c0-0000-7000-8000-000000000002",
    email: "member@example.invalid",
    role: "member",
    internet: "none",
    limits: LIMITS,
    features: { agent_teams: false },
  };
  const admin: User = {
    ...member,
    email: "admin@example.invalid",
    role: "admin",
  };

  /**
   * jsdom has no layout: puts the menu and its current item where a short window has them (top and bottom, in px,
   * against jsdom's 768 px window), and records any `scrollIntoView`, which would scroll the page as well.
   */
  function layout(menu: [number, number], item: [number, number]) {
    const had = "scrollIntoView" in Element.prototype;
    if (!had) Element.prototype.scrollIntoView = () => undefined;
    const asked: Element[] = [];
    const scroll = vi
      .spyOn(Element.prototype, "scrollIntoView")
      .mockImplementation(function (this: Element) {
        asked.push(this);
      });
    const boxes = vi
      .spyOn(Element.prototype, "getBoundingClientRect")
      .mockImplementation(function (this: Element) {
        const [top, bottom] = this.matches("nav.settings-nav")
          ? menu
          : this.getAttribute("aria-current") === "page"
            ? item
            : [0, 0];
        return DOMRect.fromRect({
          x: 0,
          y: top,
          width: 160,
          height: bottom - top,
        });
      });
    return {
      asked,
      restore() {
        boxes.mockRestore();
        scroll.mockRestore();
        if (!had)
          delete (Element.prototype as { scrollIntoView?: unknown })
            .scrollIntoView;
      },
    };
  }

  const nav = () =>
    screen.getByRole("navigation", { name: "Settings sections" });

  it("brings an administrator's section into the menu's view when the session arrives after the first render", async () => {
    // A cold load of /settings/status: no session at first, so the menu has no Status yet.
    const me = vi
      .spyOn(api, "me")
      .mockReturnValue(new Promise(() => undefined));
    const cache = new QueryClient();
    render(
      <QueryClientProvider client={cache}>
        <SettingsShell section="status">{() => null}</SettingsShell>
      </QueryClientProvider>,
    );
    expect(within(nav()).queryByRole("link", { name: "Status" })).toBeNull();
    // The menu shows from 100 px to the window's foot (768 px); Status sits below it, at 800-832 px.
    const shown = layout([100, 900], [800, 832]);
    try {
      act(() => {
        cache.setQueryData(["session"], admin);
      });
      // The query tells its observers on the next tick, as a real response would.
      expect(
        await within(nav()).findByRole("link", { name: "Status" }),
      ).toHaveAttribute("aria-current", "page");
      await waitFor(() => expect(nav().scrollTop).toBe(64));
      expect(shown.asked).toEqual([]);
    } finally {
      shown.restore();
      me.mockRestore();
    }
  });

  it("scrolls only the menu, up as well as down, and leaves a hidden menu alone", () => {
    const cache = new QueryClient();
    cache.setQueryData(["session"], member);
    const shell = (section: "tokens" | "keys") => (
      <QueryClientProvider client={cache}>
        <SettingsShell section={section}>{() => null}</SettingsShell>
      </QueryClientProvider>
    );
    const { rerender } = render(shell("tokens"));
    // Below 768 px the menu isn't drawn (no box): nothing moves.
    expect(nav().scrollTop).toBe(0);
    // The menu scrolled down earlier; the new section's item sits above its view (60-92 px; the view starts at 100).
    nav().scrollTop = 200;
    const shown = layout([100, 700], [60, 92]);
    try {
      rerender(shell("keys"));
      expect(nav().scrollTop).toBe(160);
      expect(shown.asked).toEqual([]);
    } finally {
      shown.restore();
    }
  });

  /** A member's Keys section, the session already known. */
  function showKeys(children: () => ReactNode = () => null) {
    const cache = new QueryClient();
    cache.setQueryData(["session"], member);
    return render(
      <QueryClientProvider client={cache}>
        <SettingsShell section="keys">{children}</SettingsShell>
      </QueryClientProvider>,
    );
  }

  // A passive effect runs once the browser has painted. By then the menu already shows its current item, so it never
  // moves after the page first shows.
  it("scrolls the menu before the page is painted", () => {
    const painted: number[] = [];
    function Painted() {
      useEffect(() => {
        painted.push(nav().scrollTop);
      }, []);
      return null;
    }
    // The menu shows from 100 to 700 px; Keys sits below its view, at 720-752 px.
    const shown = layout([100, 700], [720, 752]);
    try {
      showKeys(() => <Painted />);
      expect(painted).toEqual([52]);
    } finally {
      shown.restore();
    }
  });

  // Below 768 px the Section select stands in for the menu. Widening the window past it draws the menu again, and the
  // menu then shows its current item.
  it("shows the current item again when a wider window draws the menu", () => {
    const heard: (() => void)[] = [];
    vi.stubGlobal(
      "matchMedia",
      vi.fn((query: string) => ({
        matches: true,
        media: query,
        addEventListener: (_: string, listener: () => void) =>
          heard.push(listener),
        removeEventListener: vi.fn(),
      })),
    );
    try {
      // Narrow: the menu isn't drawn (no box), so nothing moves.
      showKeys();
      expect(nav().scrollTop).toBe(0);
      const shown = layout([100, 700], [720, 752]);
      try {
        act(() => {
          for (const listener of heard) listener();
        });
        expect(nav().scrollTop).toBe(52);
      } finally {
        shown.restore();
      }
    } finally {
      vi.unstubAllGlobals();
    }
  });

  // The page's fonts can arrive after the first paint and move the item; once they have, the menu shows it again.
  it("shows the current item again once the fonts have loaded", async () => {
    let loaded: () => void = () => undefined;
    const ready = new Promise<void>((resolve) => {
      loaded = resolve;
    });
    Object.defineProperty(document, "fonts", {
      configurable: true,
      value: { ready },
    });
    try {
      // Before the fonts, the item is in view (jsdom has no layout: no box).
      showKeys();
      expect(nav().scrollTop).toBe(0);
      const shown = layout([100, 700], [720, 752]);
      try {
        await act(async () => {
          loaded();
          await ready;
        });
        expect(nav().scrollTop).toBe(52);
      } finally {
        shown.restore();
      }
    } finally {
      delete (document as { fonts?: unknown }).fonts;
    }
  });
});
