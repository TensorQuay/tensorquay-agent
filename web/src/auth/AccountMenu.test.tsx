import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";

import { conversationId } from "../test/frames";

import { AccountMenu } from "./AccountMenu";

vi.mock("next/navigation", () => ({ useRouter: () => ({ replace: vi.fn() }) }));

describe("WEB-U-007 single account menu", () => {
  // The menu body is lazy; loading its module first keeps the assertions independent of import time under load.
  beforeAll(async () => {
    await import("./AccountActions");
  });
  it("keeps identity, theme and sign out behind one labelled trigger", async () => {
    const cache = new QueryClient();
    cache.setQueryData(["session"], {
      id: conversationId,
      email: "member@example.invalid",
      role: "member",
    });
    render(
      <QueryClientProvider client={cache}>
        <AccountMenu />
      </QueryClientProvider>,
    );
    const trigger = screen.getByRole("button", {
      name: "Account",
    });
    // No native title: the email names it; where the email is hidden (the rail) or cut, the app's own tooltip shows it
    // whole (UI-GUIDELINES §6; in the test's layout nothing has a size, as on the rail).
    expect(trigger).not.toHaveAttribute("title");
    fireEvent.pointerEnter(trigger);
    expect(document.querySelector(".action-tooltip")).toHaveTextContent(
      "member@example.invalid",
    );
    fireEvent.pointerLeave(trigger);
    // UI-GUIDELINES §7: the account row shows an initials avatar and the email; the menu opens upward.
    expect(within(trigger).getByText("ME")).toBeVisible();
    expect(within(trigger).getByText("member@example.invalid")).toBeVisible();
    // Theme lives only inside the account menu, never as a separate control.
    expect(screen.queryByRole("button", { name: /Theme/ })).toBeNull();
    fireEvent.click(trigger);
    const menu = await screen.findByRole("menu", { name: "Account" });
    expect(within(menu).getByText("member@example.invalid")).toBeVisible();
    expect(
      within(menu).getByRole("menuitem", { name: "Sign out" }),
    ).toBeVisible();
    expect(screen.queryByRole("dialog")).toBeNull();
    // §7 order: the email, Settings, Theme, Language, Sign out.
    const settings = within(menu).getByRole("menuitem", { name: "Settings" });
    expect(settings).toHaveAttribute("href", "/settings");
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["Settings", "Language · English", "Sign out"]);
    expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(3);
    fireEvent.click(within(menu).getByRole("menuitemradio", { name: "Dark" }));
    await waitFor(() =>
      expect(
        within(menu).getByRole("menuitemradio", { name: "Dark" }),
      ).toHaveAttribute("aria-checked", "true"),
    );
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
    fireEvent.click(trigger);
    await screen.findByRole("menu", { name: "Account" });
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
  });

  // Languages that are not fully translated say so in the menu (WEB-U-032).
  it("WEB-U-032 labels partly translated languages partial in the reader's language", async () => {
    const cache = new QueryClient();
    cache.setQueryData(["session"], {
      id: conversationId,
      email: "member@example.invalid",
      role: "member",
    });
    render(
      <QueryClientProvider client={cache}>
        <AccountMenu />
      </QueryClientProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Account" }));
    const menu = await screen.findByRole("menu", { name: "Account" });
    fireEvent.click(
      within(menu).getByRole("menuitem", { name: "Language · English" }),
    );
    const group = within(menu).getByRole("group", { name: "Language" });
    const names = within(group)
      .getAllByRole("menuitemradio")
      .map((item) => item.textContent);
    expect(names).toContain("English");
    expect(names).toContain("简体中文");
    expect(names).toContain("Français (partial)");
    const french = within(group).getByRole("menuitemradio", {
      name: "Français (partial)",
    });
    expect(french).toHaveAttribute("lang", "fr");
    expect(within(french).getByText("(partial)")).toHaveAttribute("lang", "en");
  });
});
