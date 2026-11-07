import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import { api } from "../api/client";
import { conversationId } from "../test/frames";

import { NavigationDrawer } from "./NavigationDrawer";

const replace = vi.hoisted(() => vi.fn());
vi.mock("next/navigation", () => ({
  usePathname: () => "/",
  useRouter: () => ({ replace }),
}));
// A plain anchor stands in for Next's link: a click reports both ways a component may listen for it.
vi.mock("next/link", () => ({
  default: (props: {
    href: string;
    onClick?: () => void;
    onNavigate?: () => void;
    role?: string;
    className?: string;
    children: ReactNode;
  }) => (
    <a
      href={props.href}
      role={props.role}
      className={props.className}
      onClick={(event) => {
        event.preventDefault();
        props.onClick?.();
        props.onNavigate?.();
      }}
    >
      {props.children}
    </a>
  ),
}));
vi.mock("./Navigation", () => ({
  Navigation: () => <nav aria-label="Tasks" />,
}));
afterEach(() => {
  vi.restoreAllMocks();
  replace.mockClear();
});

function drawer(onClose: () => void) {
  const cache = new QueryClient();
  cache.setQueryData(["session"], {
    id: conversationId,
    email: "member@example.invalid",
    role: "member",
  });
  render(
    <QueryClientProvider client={cache}>
      <NavigationDrawer onClose={onClose} />
    </QueryClientProvider>,
  );
  return screen.getByRole("dialog", { name: "Navigation" });
}

// The 16f page pass (F1): below 768 px the sidebar is hidden, so the drawer is the only place a phone finds the account
// menu: Settings, theme, language and sign out, at its foot as in the sidebar. An item that navigates closes the drawer.
describe("WEB-U-379 the navigation drawer has the account menu", () => {
  beforeAll(async () => {
    await import("../auth/AccountActions");
  });

  it("opens the account menu from the drawer's foot, and Settings closes the drawer", async () => {
    const onClose = vi.fn();
    const sheet = drawer(onClose);
    const trigger = within(sheet).getByRole("button", { name: "Account" });
    // At the foot, after the links, as in the sidebar.
    const links = within(sheet).getByRole("link", { name: "Channels" });
    expect(
      links.compareDocumentPosition(trigger) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    fireEvent.click(trigger);
    const menu = await within(sheet).findByRole("menu", { name: "Account" });
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["Settings", "Language · English", "Sign out"]);
    expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(3);
    // A theme choice doesn't navigate: the drawer stays open.
    fireEvent.click(within(menu).getByRole("menuitemradio", { name: "Dark" }));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Settings" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
  });

  it("signs out from the drawer, closing it on the way to the sign-in page", async () => {
    const logout = vi.spyOn(api, "logout").mockResolvedValue(undefined);
    const onClose = vi.fn();
    const sheet = drawer(onClose);
    fireEvent.click(within(sheet).getByRole("button", { name: "Account" }));
    const menu = await within(sheet).findByRole("menu", { name: "Account" });
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Sign out" }));
    await waitFor(() => expect(replace).toHaveBeenCalledWith("/login"));
    expect(logout).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
