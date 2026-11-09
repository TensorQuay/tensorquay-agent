import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { choose, chosen } from "../test/picker";

import { ChannelsLink } from "./ChannelsLink";
import { NavigationDrawer } from "./NavigationDrawer";
import { SkipLink } from "./SkipLink";
import { ThemeSelect } from "./ThemeSelect";
import { WorkspacesLink } from "./WorkspacesLink";

const pathname = vi.hoisted(() => ({ value: "/" }));
vi.mock("next/navigation", () => ({ usePathname: () => pathname.value }));
// A plain anchor stands in for Next's link, so a click reports the navigation the component asked to hear about.
vi.mock("next/link", () => ({
  default: (props: {
    href: string;
    onNavigate?: () => void;
    className?: string;
    title?: string;
    "aria-label"?: string;
    "aria-current"?: "page";
    children: ReactNode;
  }) => (
    <a
      href={props.href}
      className={props.className}
      title={props.title}
      aria-label={props["aria-label"]}
      aria-current={props["aria-current"]}
      onClick={(event) => {
        event.preventDefault();
        props.onNavigate?.();
      }}
    >
      {props.children}
    </a>
  ),
}));
// The sidebar's top block (New task, Workspaces, Channels) comes with the navigation; the task list is left out.
vi.mock("../conversations/StartProvider", () => ({
  useConversationStart: () => ({ start: () => undefined }),
}));
vi.mock("./Navigation", async () => {
  const { NavigationTop } = await import("./NavigationTop");
  return {
    Navigation: ({ onNavigate }: { onNavigate: () => void }) => (
      <NavigationTop onNavigate={onNavigate} />
    ),
  };
});
afterEach(() => {
  pathname.value = "/";
  document.documentElement.removeAttribute("data-theme");
  localStorage.clear();
});

// The shell's small parts (WEB-U-007): each does what the keyboard and the reader expect.
describe("WEB-U-007 shell parts", () => {
  it("skips past the sidebar to the main content", () => {
    render(<SkipLink />);
    expect(
      screen.getByRole("link", { name: "Skip to content" }),
    ).toHaveAttribute("href", "#main");
  });

  it("shows the chosen theme and keeps a new choice for the next visit", () => {
    render(<ThemeSelect />);
    expect(chosen("Theme")).toBe("System");
    choose("Theme", "Dark");
    expect(chosen("Theme")).toBe("Dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(localStorage.getItem("tensorquay-theme")).toBe("dark");
    choose("Theme", "System");
    expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
  });

  it("marks Workspaces as the current page only on the workspaces page", () => {
    const { rerender } = render(<WorkspacesLink />);
    const link = screen.getByRole("link", { name: "Workspaces" });
    expect(link).toHaveAttribute("href", "/workspaces");
    expect(link).not.toHaveAttribute("aria-current");
    pathname.value = "/workspaces";
    rerender(<WorkspacesLink />);
    expect(link).toHaveAttribute("aria-current", "page");
  });

  // ADR-0014: Channels sits in the sidebar beside Workspaces (UI-GUIDELINES §6; WEB-U-062).
  it("links to Channels, current on the list and on a channel's page", () => {
    pathname.value = "/";
    const { rerender } = render(<ChannelsLink />);
    const link = screen.getByRole("link", { name: "Channels" });
    expect(link).toHaveAttribute("href", "/channels");
    expect(link).not.toHaveAttribute("aria-current");
    pathname.value = "/channels";
    rerender(<ChannelsLink />);
    expect(link).toHaveAttribute("aria-current", "page");
    pathname.value = "/channels/0195d8c0-0000-7000-8000-000000000321";
    rerender(<ChannelsLink />);
    expect(link).toHaveAttribute("aria-current", "page");
  });

  it("closes the navigation drawer after following any of its links", () => {
    const onClose = vi.fn();
    // The drawer's foot holds the account menu, which reads the session (WEB-U-379).
    render(
      <QueryClientProvider client={new QueryClient()}>
        <NavigationDrawer onClose={onClose} />
      </QueryClientProvider>,
    );
    const drawer = screen.getByRole("dialog", { name: "Navigation" });
    expect(drawer).toBeVisible();
    // WEB-U-230: the drawer is the one sheet primitive, at the start edge.
    expect(drawer).toHaveClass("dialog-sheet");
    expect(drawer).toHaveAttribute("data-placement", "start");
    fireEvent.click(screen.getByRole("link", { name: "New task" }));
    fireEvent.click(screen.getByRole("link", { name: "Workspaces" }));
    fireEvent.click(screen.getByRole("link", { name: "Channels" }));
    expect(onClose).toHaveBeenCalledTimes(3);
  });
});
