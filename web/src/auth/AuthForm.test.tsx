import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { api, ApiError } from "../api/client";
import { conversationId } from "../test/frames";
import { LIMITS } from "../test/user";

import { AuthForm } from "./AuthForm";
import { returnPath } from "./return-path";

const replace = vi.fn();
vi.mock("next/navigation", () => ({ useRouter: () => ({ replace }) }));
beforeEach(() => {
  replace.mockReset();
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(Response.json({ required: false })),
  );
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
function page(mode: "login" | "setup") {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <AuthForm mode={mode} />
    </QueryClientProvider>,
  );
  fireEvent.change(screen.getByLabelText("Email"), {
    target: { value: "admin@example.invalid" },
  });
  fireEvent.change(screen.getByLabelText("Password"), {
    target: { value: "synthetic-test-password" },
  });
}

describe("WEB-U-007 authentication forms", () => {
  it.each(["login", "setup"] as const)(
    "shows %s preparation failures without losing input",
    async (mode) => {
      vi.stubGlobal(
        "FormData",
        class {
          constructor() {
            throw new Error("Could not prepare the form.");
          }
        },
      );
      page(mode);
      if (mode === "setup")
        fireEvent.change(screen.getByLabelText("Setup token"), {
          target: { value: "synthetic-bootstrap" },
        });
      fireEvent.click(
        screen.getByRole("button", {
          name: mode === "login" ? "Sign in" : "Create administrator",
        }),
      );
      expect(await screen.findByRole("alert")).toHaveTextContent(
        "Could not prepare the form.",
      );
      expect(screen.getByLabelText("Email")).toHaveValue(
        "admin@example.invalid",
      );
      expect(screen.getByLabelText("Password")).toHaveValue(
        "synthetic-test-password",
      );
    },
  );
  it("shows throttling with the retry delay without losing the entered email", async () => {
    vi.spyOn(api, "login").mockRejectedValue(
      new ApiError(429, "rate_limited", "Too many attempts.", 12),
    );
    page("login");
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Too many attempts. Try again in 12 s.",
    );
    expect(screen.getByLabelText("Email")).toHaveValue("admin@example.invalid");
  });
  it("signs the first administrator in and clears credentials without another form", async () => {
    const bootstrap = vi.spyOn(api, "bootstrap").mockResolvedValue({
      id: conversationId,
      email: "admin@example.invalid",
      role: "admin",
      internet: "none",
      limits: LIMITS,
      features: { agent_teams: false },
    });
    page("setup");
    fireEvent.change(screen.getByLabelText("Setup token"), {
      target: { value: "synthetic-bootstrap" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Create administrator" }),
    );
    await screen.findByRole("button", { name: "Create administrator" });
    await waitFor(() => expect(replace).toHaveBeenCalledWith("/"));
    expect(screen.getByLabelText("Password", { exact: true })).toHaveValue("");
    expect(screen.getByLabelText("Setup token")).toHaveValue("");
    expect(bootstrap.mock.calls[0]?.[0]).toEqual({
      email: "admin@example.invalid",
      password: "synthetic-test-password",
      token: "synthetic-bootstrap",
    });
  });
  it("shows live password rules and a labelled visibility toggle", () => {
    page("setup");
    const input = screen.getByLabelText("Password", { exact: true });
    expect(screen.getByLabelText("At least 12 characters: met")).toBeVisible();
    expect(
      screen.getByLabelText("Does not contain your email: met"),
    ).toBeVisible();
    fireEvent.change(input, { target: { value: "short" } });
    expect(
      screen.getByLabelText("At least 12 characters: not met"),
    ).toBeVisible();
    fireEvent.change(input, {
      target: { value: "admin@example.invalid-password" },
    });
    expect(
      screen.getByLabelText("Does not contain your email: not met"),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Show password" }));
    expect(input).toHaveAttribute("type", "text");
    fireEvent.click(screen.getByRole("button", { name: "Hide password" }));
    expect(input).toHaveAttribute("type", "password");
    expect(document.getElementById("setup-help")).toHaveTextContent(
      // WEB-U-371 (a first-hour check before 17.0): the help names the file the install writes, not an installer
      // output.
      /^The setup token is TQA_BOOTSTRAP_TOKEN in compose\.env, in the folder you installed into\.$/,
    );
  });
  it("redirects sign-in to setup only when the setup contract requires it", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(Response.json({ required: true })),
    );
    page("login");
    await waitFor(() => expect(replace).toHaveBeenCalledWith("/setup"));
  });
  it("turns authentication failure into useful sign-in copy", async () => {
    vi.spyOn(api, "login").mockRejectedValue(
      new ApiError(401, "unauthenticated", "authentication required"),
    );
    page("login");
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Email or password is incorrect",
    );
    // WEB-U-380: the refusal is said on the password, which is marked and focused again once the form is enabled
    // (a pass over the pages before 17.0); a new email clears it.
    const password = screen.getByLabelText("Password", { exact: true });
    await waitFor(() => expect(password).toHaveFocus());
    expect(password).toHaveAttribute("aria-invalid", "true");
    expect(password).toHaveAccessibleDescription(
      expect.stringContaining("Email or password is incorrect") as string,
    );
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    fireEvent.change(screen.getByLabelText("Email"), {
      target: { value: "other@example.invalid" },
    });
    expect(password).not.toHaveAttribute("aria-invalid");
    expect(screen.queryByRole("alert")).toBeNull();
  });
  it("explains a wrong setup token without the gateway message and keeps the input", async () => {
    vi.spyOn(api, "bootstrap").mockRejectedValue(
      new ApiError(401, "unauthorized", "authentication required"),
    );
    page("setup");
    fireEvent.change(screen.getByLabelText("Setup token"), {
      target: { value: "synthetic-wrong-token" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Create administrator" }),
    );
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      "That setup token is not correct. Copy it again from compose.env.",
    );
    expect(alert).not.toHaveTextContent(/authentication required/i);
    // WEB-U-179 (the 13f screens, smaller items): the variable is named once, in the help that says where the token is.
    expect(alert).not.toHaveTextContent("TQA_BOOTSTRAP_TOKEN");
    expect(document.body.textContent?.split("TQA_BOOTSTRAP_TOKEN").length).toBe(
      2,
    );
    expect(screen.getByLabelText("Setup token")).toHaveValue(
      "synthetic-wrong-token",
    );
    expect(screen.getByLabelText("Email")).toHaveValue("admin@example.invalid");
    expect(replace).not.toHaveBeenCalled();
    // WEB-U-380: the token field carries the refusal, marked and focused again (a pass over the pages before 17.0);
    // typing a new token clears it.
    const token = screen.getByLabelText("Setup token");
    await waitFor(() => expect(token).toHaveFocus());
    expect(token).toHaveAttribute("aria-invalid", "true");
    expect(token).toHaveAccessibleDescription(
      expect.stringContaining("That setup token is not correct") as string,
    );
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    fireEvent.change(token, { target: { value: "synthetic-other-token" } });
    expect(token).not.toHaveAttribute("aria-invalid");
  });
  it("keeps other failures in the form's message, with no field marked", async () => {
    vi.spyOn(api, "login").mockRejectedValue(
      new ApiError(429, "rate_limited", "too many attempts"),
    );
    page("login");
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(
      screen.getByLabelText("Password", { exact: true }),
    ).not.toHaveAttribute("aria-invalid");
    expect(screen.getByLabelText("Email")).not.toHaveAttribute("aria-invalid");
  });
  it("accepts only an app-local return path", () => {
    // A pass over the pages before 17.0: the app's own pages, so a person signed out on Workspaces, a channel or a
    // settings page comes back to it, not to New task.
    for (const path of [
      "/",
      `/c/${conversationId}`,
      "/workspaces",
      "/channels",
      "/channels/0195d8c0-0000-7000-8000-000000000009",
      "/settings",
      "/settings/models/qwen3.4b",
      "/settings/keys/shared/new",
    ])
      expect(returnPath(path)).toBe(path);
    // A plain segment that was encoded comes back decoded, as the page's own link would read.
    expect(returnPath("/settings/%6Dodels")).toBe("/settings/models");
    for (const path of [
      null,
      "/settings/..",
      "/settings/../api/v1/me",
      "/settings/a/b/c/d",
      "/workspaces/x",
      "/channels/a/b",
      "/design",
      "/settings//example.invalid",
      // Encoded, as a browser reads it: %2e%2e is "..", so these would land on /api or the start.
      "/settings/%2e%2e/api",
      "/settings/%2E%2e",
      "/settings/%2e",
      "/settings/%252e%252e/api",
      "/settings/models/%E0%A4%A",
      "/c/%2F%2Fexample.invalid",
      "/settings/~x",
      "https://example.invalid",
      "//example.invalid",
      "/\\example.invalid",
      "javascript:alert(1)",
      "/login?next=/",
      "/api/v1/me",
      "/\n/example.invalid",
      "/settings/.hidden",
      "/settings/models\n",
      "/settingsx",
    ])
      expect(returnPath(path)).toBe("/");
  });

  // TEST: WEB-U-257
  // WEB-U-257 (a pass over the pages before 17.0): after an expired session, sign-in brought a person back only to New
  // task or a task; from Workspaces, Settings or a channel they landed on New task. The app's own pages come back now.
  it("returns to the app's own pages, without their query", () => {
    for (const path of [
      "/workspaces",
      "/channels",
      "/channels/0199a1b2-c3d4",
      "/settings",
      "/settings/models",
      "/settings/models/local-small-mac",
      "/settings/agent-profiles/0199a1b2/duplicate",
      "/settings/keys/shared/new",
    ])
      expect(returnPath(path)).toBe(path);
    expect(returnPath("/workspaces?filter=mine")).toBe("/workspaces");
    expect(returnPath("/?q=1")).toBe("/");
  });
});

// WEB-U-355 (a pass over the pages before 17.0): sign-in and setup say an empty or malformed field under it in the
// app's words, never the browser's own bubble, and send nothing.
describe("WEB-U-355 sign-in and setup fields, said by the page", () => {
  function empty(mode: "login" | "setup") {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <AuthForm mode={mode} />
      </QueryClientProvider>,
    );
  }
  const submit = () =>
    fireEvent.click(
      screen.getByRole("button", { name: /^(Sign in|Create administrator)$/ }),
    );

  it("signing in with nothing typed says what's missing", async () => {
    const login = vi.spyOn(api, "login");
    empty("login");
    expect(screen.getByLabelText("Email").closest("form")).toHaveAttribute(
      "novalidate",
    );
    submit();
    expect(await screen.findByText("Enter your email.")).toBeInTheDocument();
    expect(screen.getByText("Enter your password.")).toBeInTheDocument();
    expect(screen.getByLabelText("Email")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
    expect(login).not.toHaveBeenCalled();
  });

  it("a malformed email is said, and fixing it clears it", async () => {
    const login = vi.spyOn(api, "login");
    empty("login");
    fireEvent.change(screen.getByLabelText("Email"), {
      target: { value: "ana" },
    });
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "synthetic-test-password" },
    });
    submit();
    expect(
      await screen.findByText(
        "Enter an email address, such as ana@example.com.",
      ),
    ).toBeInTheDocument();
    expect(login).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Email"), {
      target: { value: "ana@example.invalid" },
    });
    expect(
      screen.queryByText("Enter an email address, such as ana@example.com."),
    ).toBeNull();
  });

  it("setup says a short password and a missing setup token", async () => {
    const bootstrap = vi.spyOn(api, "bootstrap");
    empty("setup");
    fireEvent.change(screen.getByLabelText("Email"), {
      target: { value: "admin@example.invalid" },
    });
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "short" },
    });
    submit();
    expect(
      await screen.findByText("Use at least 12 characters."),
    ).toBeInTheDocument();
    expect(screen.getByText("Enter the setup token.")).toBeInTheDocument();
    expect(bootstrap).not.toHaveBeenCalled();
  });
});
