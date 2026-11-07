import { runInNewContext } from "node:vm";

import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { THEME_SCRIPT } from "./theme";
import { ThemeToggle } from "./ThemeToggle";

beforeEach(() => {
  document.documentElement.removeAttribute("data-theme");
  localStorage.clear();
});
afterEach(() => vi.restoreAllMocks());

describe("ThemeToggle", () => {
  it("selects system, light and dark with the keyboard", () => {
    render(<ThemeToggle />);
    const toggle = screen.getByRole("button", { name: "Theme: System" });
    fireEvent.keyDown(toggle, { key: "ArrowRight" });
    expect(toggle).toHaveAccessibleName("Theme: Light");
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    fireEvent.keyDown(toggle, { key: "ArrowRight" });
    expect(toggle).toHaveAccessibleName("Theme: Dark");
    expect(localStorage.getItem("tensorquay-theme")).toBe("dark");
    fireEvent.keyDown(toggle, { key: "ArrowRight" });
    expect(toggle).toHaveAccessibleName("Theme: System");
    expect(document.documentElement).not.toHaveAttribute("data-theme");
  });
  it("still changes theme when storage writes fail", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("Storage unavailable");
    });
    render(<ThemeToggle />);
    fireEvent.click(screen.getByRole("button", { name: "Theme: System" }));
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
  });
  it("applies a stored preference before hydration and tolerates blocked reads", () => {
    localStorage.setItem("tensorquay-theme", "dark");
    const bootstrap = () => {
      runInNewContext(THEME_SCRIPT, { localStorage, document });
    };
    bootstrap();
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("Storage unavailable");
    });
    expect(() => bootstrap()).not.toThrow();
  });
});
