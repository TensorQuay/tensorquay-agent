import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { NavigationTop } from "./NavigationTop";
import { Shell } from "./Shell";

vi.mock("next/navigation", () => ({ usePathname: () => "/workspaces" }));
vi.mock("next/link", () => ({
  default: ({
    href,
    className,
    children,
    ...rest
  }: {
    href: string;
    className?: string;
    children: ReactNode;
    "aria-label"?: string;
    "aria-current"?: "page";
    title?: string;
    onPointerEnter?: () => void;
    onPointerLeave?: () => void;
  }) => (
    <a
      href={href}
      className={className}
      aria-label={rest["aria-label"]}
      aria-current={rest["aria-current"]}
      title={rest.title}
      onPointerEnter={rest.onPointerEnter}
      onPointerLeave={rest.onPointerLeave}
    >
      {children}
    </a>
  ),
}));
vi.mock("../conversations/StartProvider", () => ({
  useConversationStart: () => ({ start: () => undefined }),
}));
// The task list and the account menu are tested on their own; here they mark their places.
vi.mock("../conversations/ConversationList", () => ({
  ConversationList: () => <section aria-label="Tasks" />,
}));
vi.mock("../auth/AccountMenu", () => ({
  AccountMenu: () => <button type="button">Account</button>,
}));
vi.mock("../conversations/TaskSearchHost", () => ({
  TaskSearchHost: () => null,
}));
afterEach(() => vi.restoreAllMocks());

// WEB-U-452 (UI-GUIDELINES §6; decided on 9 Nov 2026): the sidebar's top block is New task, Workspaces and Channels, in
// that order, before the task list, and its foot is the account alone. An item whose label shows has no native tooltip;
// where its label is hidden (the rail), the app's own tooltip names it.
describe("WEB-U-452 the sidebar's top block", () => {
  it("lists New task, Workspaces and Channels in that order, before the task list, the account alone at the foot", () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <Shell>
          <p>Page</p>
        </Shell>
      </QueryClientProvider>,
    );
    const sidebar = screen.getByRole("complementary");
    const navigation = within(sidebar).getByRole("navigation");
    const links = within(navigation).getAllByRole("link");
    expect(links.map((link) => link.getAttribute("aria-label"))).toEqual([
      "New task",
      "Workspaces",
      "Channels",
    ]);
    const list = within(navigation).getByRole("region", { name: "Tasks" });
    for (const link of links)
      expect(
        link.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
    // Workspaces and Channels appear once: the foot keeps only the account.
    expect(
      within(sidebar).getAllByRole("link", { name: "Workspaces" }),
    ).toHaveLength(1);
    expect(sidebar.querySelector(".sidebar-footer")).toBeNull();
    const account = within(sidebar).getByRole("button", { name: "Account" });
    expect(
      navigation.compareDocumentPosition(account) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    // Workspaces is the current page here; its row says so.
    expect(links[1]).toHaveAttribute("aria-current", "page");
  });

  it("gives no item a native tooltip where its label shows", () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <Shell>
          <p>Page</p>
        </Shell>
      </QueryClientProvider>,
    );
    const sidebar = screen.getByRole("complementary");
    for (const name of [
      "TensorQuay Agent",
      "New task",
      "Workspaces",
      "Channels",
    ])
      expect(within(sidebar).getByRole("link", { name })).not.toHaveAttribute(
        "title",
      );
  });

  it("names an item with the app's own tooltip where its label is hidden, and drops it on leaving", () => {
    render(<NavigationTop />);
    const workspaces = screen.getByRole("link", { name: "Workspaces" });
    // In the test's layout nothing has a size, as a label hidden on the rail has none.
    fireEvent.pointerEnter(workspaces);
    const tip = document.querySelector(".action-tooltip");
    expect(tip).toHaveTextContent("Workspaces");
    expect(tip).toHaveAttribute("aria-hidden", "true");
    fireEvent.pointerLeave(workspaces);
    expect(document.querySelector(".action-tooltip")).toBeNull();
  });
});
