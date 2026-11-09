import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

import { fireEvent, render, screen, within } from "@testing-library/react";
import { useState, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { KeysViews } from "../settings/KeysViews";

import { TabList, tabPanelIds } from "./Tabs";

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
    "aria-current"?: "page";
  }) => (
    <a href={href} className={className} aria-current={rest["aria-current"]}>
      {children}
    </a>
  ),
}));

type View = "files" | "changes" | "context";
const VIEWS = [
  { value: "files", label: "Files" },
  { value: "changes", label: "Changes" },
  { value: "context", label: "Context" },
] as const;

/** A place with three views, as the Files panel has, its panel named from the tab list's ids. */
function Views() {
  const [current, setCurrent] = useState<View>("files");
  return (
    <>
      <TabList
        id="panel"
        label="Task views"
        views={VIEWS}
        current={current}
        onChoose={setCurrent}
        className="files-tabs"
      />
      <div role="tabpanel" {...tabPanelIds("panel", current)}>
        {current}
      </div>
    </>
  );
}

const tab = (name: string) => screen.getByRole("tab", { name });

function sources(directory: string): string[] {
  return readdirSync(directory).flatMap((name) => {
    const path = join(directory, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.tsx$/.test(name) && !/\.test\.tsx$/.test(name) ? [path] : [];
  });
}

// WEB-U-454 (UI-GUIDELINES §7 Tabs, 9 Nov 2026): one tab list for the whole app, as WAI-ARIA tabs: labelled,
// the chosen tab selected and the list's one Tab stop; the arrow keys choose the next or previous and wrap, Home and End
// the ends, the focus moving with them; each tab names the panel it controls. The Files panel and a channel's side use
// it; Keys' Mine | Shared, links to two pages, keep their links and take its look.
describe("WEB-U-454 the one tab list", () => {
  it("is a labelled tab list whose chosen tab is selected and names its panel", () => {
    render(<Views />);
    const list = screen.getByRole("tablist", { name: "Task views" });
    expect(
      within(list)
        .getAllByRole("tab")
        .map((each) => each.textContent),
    ).toEqual(["Files", "Changes", "Context"]);
    expect(tab("Files")).toHaveAttribute("aria-selected", "true");
    expect(tab("Changes")).toHaveAttribute("aria-selected", "false");
    expect(tab("Files")).toHaveAttribute("aria-controls", "panel-files-panel");
    expect(screen.getByRole("tabpanel", { name: "Files" })).toHaveAttribute(
      "id",
      "panel-files-panel",
    );
  });

  it("keeps one Tab stop: the chosen tab, or the first when none of them is chosen", () => {
    const { rerender } = render(
      <TabList
        id="panel"
        label="Task views"
        views={VIEWS}
        current="changes"
        onChoose={() => undefined}
      />,
    );
    expect(screen.getAllByRole("tab").map((each) => each.tabIndex)).toEqual([
      -1, 0, -1,
    ]);
    rerender(
      <TabList
        id="panel"
        label="Task views"
        views={VIEWS.slice(0, 2)}
        current="context"
        onChoose={() => undefined}
      />,
    );
    expect(screen.getAllByRole("tab").map((each) => each.tabIndex)).toEqual([
      0, -1,
    ]);
  });

  it("chooses with the arrow keys, wrapping, and with Home and End, the focus moving along", () => {
    render(<Views />);
    tab("Files").focus();
    fireEvent.keyDown(tab("Files"), { key: "ArrowRight" });
    expect(tab("Changes")).toHaveAttribute("aria-selected", "true");
    expect(tab("Changes")).toHaveFocus();
    fireEvent.keyDown(tab("Changes"), { key: "End" });
    expect(tab("Context")).toHaveAttribute("aria-selected", "true");
    expect(tab("Context")).toHaveFocus();
    fireEvent.keyDown(tab("Context"), { key: "ArrowRight" });
    expect(tab("Files")).toHaveFocus();
    fireEvent.keyDown(tab("Files"), { key: "ArrowLeft" });
    expect(tab("Context")).toHaveFocus();
    fireEvent.keyDown(tab("Context"), { key: "Home" });
    expect(tab("Files")).toHaveFocus();
    expect(tab("Files")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel", { name: "Files" })).toHaveTextContent(
      "files",
    );
  });

  it("keeps the page still for the keys it handles and leaves the others alone", () => {
    render(<Views />);
    for (const key of ["ArrowRight", "ArrowLeft", "Home", "End"])
      expect(
        fireEvent.keyDown(screen.getByRole("tab", { selected: true }), {
          key,
        }),
      ).toBe(false);
    const chosen = screen.getByRole("tab", { selected: true });
    expect(fireEvent.keyDown(chosen, { key: "ArrowDown" })).toBe(true);
    expect(fireEvent.keyDown(chosen, { key: "a" })).toBe(true);
    expect(screen.getByRole("tab", { selected: true })).toBe(chosen);
  });

  it("chooses a tab on a click", () => {
    render(<Views />);
    fireEvent.click(tab("Context"));
    expect(tab("Context")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel", { name: "Context" })).toBeVisible();
  });

  it("draws every tab in the one style; a place adds only its own layout class", () => {
    render(<Views />);
    expect(screen.getByRole("tablist")).toHaveClass("tab-list", "files-tabs");
    for (const each of screen.getAllByRole("tab"))
      expect(each.className).toBe("tab");
  });

  it("is the app's only tab list: the Files panel and a channel's side use it", () => {
    const lists = sources("src").filter((path) =>
      readFileSync(path, "utf8").includes('role="tablist"'),
    );
    expect(lists).toEqual([join("src", "ui", "Tabs.tsx")]);
    for (const path of [
      "src/files/PanelTabs.tsx",
      "src/channels/ChannelSide.tsx",
    ])
      expect(readFileSync(path, "utf8")).toMatch(/<TabList\b/);
  });

  it("draws Keys' Mine | Shared, links to two pages, as the same tabs", () => {
    render(<KeysViews current="shared" />);
    const nav = screen.getByRole("navigation", { name: "Keys" });
    expect(nav).toHaveClass("tab-list");
    const links = within(nav).getAllByRole("link");
    expect(links.map((link) => link.className)).toEqual(["tab", "tab"]);
    expect(within(nav).getByRole("link", { name: "Shared" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(within(nav).getByRole("link", { name: "Mine" })).not.toHaveAttribute(
      "aria-current",
    );
  });
});
