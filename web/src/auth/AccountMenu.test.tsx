import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";

import { conversationId } from "../test/frames";

import { AccountMenu } from "./AccountMenu";

vi.mock("next/navigation", () => ({ useRouter: () => ({ replace: vi.fn() }) }));

describe("WEB-U-007 single account menu", () => {
  // The menu body is lazy; loading its module first keeps the assertions independent of import time under load.
  beforeAll(async () => {
    await import("./AccountActions");
  });
  it("WEB-U-463 keeps identity, the places people reach for and sign out behind one labelled trigger", async () => {
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
    expect(trigger).toHaveAttribute("title", "member@example.invalid");
    // UI-GUIDELINES §7: the account row shows an initials avatar and the email; the menu opens upward.
    expect(within(trigger).getByText("ME")).toBeVisible();
    expect(within(trigger).getByText("member@example.invalid")).toBeVisible();
    fireEvent.click(trigger);
    const menu = await screen.findByRole("menu", { name: "Account" });
    // Who is signed in, with their role.
    expect(
      within(menu).getByText("member@example.invalid · Member"),
    ).toBeVisible();
    // The order: Settings, Language, Help, Send feedback | API tokens, About | Sign out. Theme is in Settings › Account.
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual([
      "Settings",
      "Language · English",
      "Help (opens in a new tab)",
      "Send feedback (opens in a new tab)",
      "API tokens",
      "About",
      "Sign out",
    ]);
    expect(within(menu).queryAllByRole("menuitemradio")).toHaveLength(0);
    expect(within(menu).getAllByRole("separator")).toHaveLength(2);
    expect(
      within(menu).getByRole("menuitem", { name: "Settings" }),
    ).toHaveAttribute("href", "/settings");
    expect(
      within(menu).getByRole("menuitem", { name: "API tokens" }),
    ).toHaveAttribute("href", "/settings/tokens");
    // The project's pages open in a new tab, and say so.
    const help = within(menu).getByRole("menuitem", { name: /^Help/ });
    expect(help).toHaveAccessibleName("Help (opens in a new tab)");
    expect(help).toHaveAttribute("target", "_blank");
    expect(help).toHaveAttribute("rel", "noopener noreferrer");
    expect(help.getAttribute("href")).toMatch(
      /^https:\/\/github\.com\/TensorQuay\/tensorquay-agent\//,
    );
    // About opens its own group: what's new, the source and the licence.
    fireEvent.click(within(menu).getByRole("menuitem", { name: "About" }));
    const about = within(menu).getByRole("group", { name: "About" });
    expect(
      within(about)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual([
      "What’s new (opens in a new tab)",
      "Source code (opens in a new tab)",
      "Licence (opens in a new tab)",
    ]);
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
    fireEvent.click(trigger);
    await screen.findByRole("menu", { name: "Account" });
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("WEB-U-463 adds Usage for an administrator", async () => {
    const cache = new QueryClient();
    cache.setQueryData(["session"], {
      id: conversationId,
      email: "admin@example.invalid",
      role: "admin",
    });
    render(
      <QueryClientProvider client={cache}>
        <AccountMenu />
      </QueryClientProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Account" }));
    const menu = await screen.findByRole("menu", { name: "Account" });
    expect(
      within(menu).getByText("admin@example.invalid · Administrator"),
    ).toBeVisible();
    expect(
      within(menu).getByRole("menuitem", { name: "Usage" }),
    ).toHaveAttribute("href", "/settings/usage");
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
