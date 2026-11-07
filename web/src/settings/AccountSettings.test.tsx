import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { choose, chosen, picker } from "../test/picker";
import { LIMITS } from "../test/user";

import { AccountSettings } from "./AccountSettings";

afterEach(() => {
  delete document.documentElement.dataset.theme;
  try {
    localStorage.clear();
  } catch {
    /* no storage */
  }
});

// WEB-U-078: Your account › Account: who you are, and the theme and language the account menu also sets.
describe("WEB-U-078 the account page", () => {
  it("shows the email and role, and sets the theme and the language", () => {
    render(
      <AccountSettings
        me={{
          id: "0195d8c0-0000-7000-8000-000000000002",
          email: "member@example.invalid",
          role: "member",
          internet: "none",
          limits: LIMITS,
          features: { agent_teams: false },
        }}
      />,
    );
    expect(screen.getByText("member@example.invalid")).toBeInTheDocument();
    expect(screen.getByText("Member")).toBeInTheDocument();
    choose("Theme", "Dark");
    expect(chosen("Theme")).toBe("Dark");
    expect(chosen("Language")).toBe("English");
    fireEvent.click(picker("Language"));
    expect(
      screen.getByRole("menuitemradio", { name: /^简体中文/ }),
    ).toBeInTheDocument();
  });
});
