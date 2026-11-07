import { readFileSync } from "node:fs";
import { join } from "node:path";

import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { NotFoundPage } from "./NotFoundPage";

vi.mock("next/link", () => ({
  default: ({
    href,
    children,
    ...rest
  }: {
    href: string;
    children: ReactNode;
  } & Record<string, unknown>) => (
    <a href={href} {...rest}>
      {children}
    </a>
  ),
}));

const zh = (
  JSON.parse(
    readFileSync(join("src", "i18n", "locales", "zh-Hans.json"), "utf8"),
  ) as { messages: Record<string, string> }
).messages;

// TEST: WEB-U-381
describe("the app's own 404 page", () => {
  it("says the page isn't there in the product's words, with one way on to the tasks", () => {
    render(<NotFoundPage />);
    expect(
      screen.getByRole("heading", { level: 1, name: "Page not found" }),
    ).toBeVisible();
    expect(
      screen.getByText(
        "There's nothing at this address. The link may be wrong, or the page may have moved.",
      ),
    ).toBeVisible();
    const home = screen.getByRole("link", { name: "Go to your tasks" });
    expect(home).toHaveAttribute("href", "/");
    expect(home).toHaveAttribute("data-variant", "primary");
    expect(screen.getByRole("img", { name: "TensorQuay" })).toBeVisible();
    // The skip link's target: the page is its own main, as the sign-in pages are.
    expect(screen.getByRole("main")).toHaveAttribute("id", "main");
    expect(document.title).toBe("Page not found · TensorQuay Agent");
  });

  it("has its words in Chinese, so the page follows the reader's language", () => {
    for (const key of [
      "notFound.tab",
      "notFound.title",
      "notFound.body",
      "notFound.home",
    ])
      expect(zh[key], key).toMatch(/[一-鿿]/);
    expect(zh["notFound.home"]).toBe(zh["task.goToTasks"]);
  });
});
