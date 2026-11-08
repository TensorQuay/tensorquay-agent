import { runInNewContext } from "node:vm";

import { act, fireEvent, render, screen } from "@testing-library/react";
import { renderToString } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SIDEBAR_SCRIPT } from "./sidebar";
import { SidebarToggle } from "./SidebarToggle";

function viewport(narrow: boolean) {
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) => ({
      matches: narrow,
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  );
}
beforeEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset.sidebar;
  viewport(false);
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

// UI-GUIDELINES §6 layouts A and C: expanded by default, a rail on request, remembered per browser, key [.
describe("WEB-U-007 sidebar toggle", () => {
  it("collapses the default expanded sidebar to a rail and remembers it", () => {
    render(<SidebarToggle />);
    fireEvent.click(screen.getByRole("button", { name: "Collapse sidebar" }));
    expect(document.documentElement.dataset.sidebar).toBe("rail");
    expect(localStorage.getItem("tensorquay-sidebar")).toBe("rail");
    fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
    expect(document.documentElement.dataset.sidebar).toBe("expanded");
    expect(localStorage.getItem("tensorquay-sidebar")).toBe("expanded");
  });

  it("starts as a rail between 768 and 1024 px when nothing was chosen", () => {
    viewport(true);
    render(<SidebarToggle />);
    expect(
      screen.getByRole("button", { name: "Expand sidebar" }),
    ).toBeVisible();
  });

  // Before hydration the server can't know the width or the stored choice: the stylesheet shows the layout, so the
  // toggle carries both names and the stylesheet's own rule hides the one that doesn't apply.
  it("leaves its name to the stylesheet's layout before hydration", () => {
    const host = document.createElement("div");
    host.innerHTML = renderToString(<SidebarToggle />);
    const button = host.querySelector("button");
    expect(button).not.toHaveAttribute("aria-label");
    expect(button).not.toHaveAttribute("title");
    const names = [...host.querySelectorAll("[data-sidebar-for]")];
    expect(
      names.map((name) => [
        name.getAttribute("data-sidebar-for"),
        name.textContent,
        name.hasAttribute("hidden"),
      ]),
    ).toEqual([
      ["expanded", "Collapse sidebar", false],
      ["rail", "Expand sidebar", false],
    ]);
  });

  it("names itself and offers its key in its tooltip once it knows the layout", () => {
    viewport(true);
    render(<SidebarToggle />);
    expect(
      screen.getByRole("button", { name: "Expand sidebar" }),
    ).toHaveAttribute("title", "Expand sidebar ([)");
  });

  it("restores the remembered choice before hydration", () => {
    localStorage.setItem("tensorquay-sidebar", "rail");
    runInNewContext(SIDEBAR_SCRIPT, { localStorage, document });
    expect(document.documentElement.dataset.sidebar).toBe("rail");
    localStorage.setItem("tensorquay-sidebar", "sideways");
    delete document.documentElement.dataset.sidebar;
    runInNewContext(SIDEBAR_SCRIPT, { localStorage, document });
    expect(document.documentElement.dataset.sidebar).toBeUndefined();
  });

  it("toggles with [ except while the user is typing", () => {
    render(
      <>
        <SidebarToggle />
        <textarea aria-label="Message" />
      </>,
    );
    act(() => {
      fireEvent.keyDown(document.body, { key: "[" });
    });
    expect(
      screen.getByRole("button", { name: "Expand sidebar" }),
    ).toBeVisible();
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Message" }), {
      key: "[",
    });
    expect(
      screen.getByRole("button", { name: "Expand sidebar" }),
    ).toBeVisible();
    act(() => {
      fireEvent.keyDown(document.body, { key: "[", ctrlKey: true });
    });
    expect(
      screen.getByRole("button", { name: "Expand sidebar" }),
    ).toBeVisible();
  });

  it("still toggles the current tab when storage is unavailable", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("Storage is disabled");
    });
    render(<SidebarToggle />);
    fireEvent.click(screen.getByRole("button", { name: "Collapse sidebar" }));
    expect(
      screen.getByRole("button", { name: "Expand sidebar" }),
    ).toBeVisible();
  });
});
