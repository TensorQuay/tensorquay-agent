import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { LIMITS } from "../test/user";

import { JoinForm } from "./JoinForm";

const replace = vi.fn();
vi.mock("next/navigation", () => ({ useRouter: () => ({ replace }) }));
const invitee = {
  email: "new@example.invalid",
  role: "member",
  expires_at: "2026-01-08T00:00:00Z",
};
const problem = (status: number, code: string, headers = {}) =>
  Promise.resolve(
    Response.json(
      { error: { code, message: "diagnostic", retryable: false } },
      { status, headers },
    ),
  );
const fetch = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  replace.mockReset();
  fetch.mockReset();
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => {
  vi.unstubAllGlobals();
});
function show(token = "synthetic-invitation-token") {
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <JoinForm token={token} />
    </QueryClientProvider>,
  );
}

// Task 067 / GW-I-002: joining by an email-bound invitation link; every unusable link is one plain state.
describe("WEB-U-023 join", () => {
  it("shows whom the link is for and joins with a password", async () => {
    fetch
      .mockImplementationOnce(() => Promise.resolve(Response.json(invitee)))
      .mockImplementationOnce(() =>
        Promise.resolve(
          Response.json(
            {
              id: "0195d8c0-0000-7000-8000-000000000003",
              email: invitee.email,
              role: invitee.role,
              internet: "none",
              limits: LIMITS,
              features: { agent_teams: false },
            },
            { status: 201 },
          ),
        ),
      );
    show();
    expect(
      await screen.findByRole("heading", { name: "Join TensorQuay Agent" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByText("You’re invited as a member."),
    ).toBeInTheDocument();
    const email = screen.getByRole("textbox", { name: "Email" });
    expect(email).toHaveValue("new@example.invalid");
    expect(email).toHaveAttribute("readonly");
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "synthetic-joining-password" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Join" }));
    await vi.waitFor(() => expect(replace).toHaveBeenCalledWith("/"));
    expect(fetch.mock.calls[0]?.[1]?.body).toBe(
      JSON.stringify({ token: "synthetic-invitation-token" }),
    );
    expect(fetch.mock.calls[1]?.[1]?.body).toBe(
      JSON.stringify({
        token: "synthetic-invitation-token",
        password: "synthetic-joining-password",
      }),
    );
  });

  it("shows one plain state for any unusable link", async () => {
    fetch.mockImplementationOnce(() => problem(400, "invitation_invalid"));
    show();
    expect(
      await screen.findByText(
        "This invitation link is not valid. It may have expired, been used already or been revoked. Ask your administrator for a new one.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute(
      "href",
      "/login",
    );
    expect(screen.queryByLabelText("Password")).toBeNull();
  });

  it("turns a link that stopped working before Join into the same state", async () => {
    fetch
      .mockImplementationOnce(() => Promise.resolve(Response.json(invitee)))
      .mockImplementationOnce(() => problem(400, "invitation_invalid"));
    show();
    fireEvent.change(await screen.findByLabelText("Password"), {
      target: { value: "synthetic-joining-password" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Join" }));
    expect(
      await screen.findByText(/This invitation link is not valid\./),
    ).toBeInTheDocument();
    expect(replace).not.toHaveBeenCalled();
  });

  it("asks to wait when rate limited", async () => {
    fetch.mockImplementation(() =>
      problem(429, "rate_limited", { "retry-after": "30" }),
    );
    show();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Too many attempts.",
    );
  });
});
// WEB-U-355 (the 16f page pass, E1): a short password is said under the field in the app's words, never the
// browser's own bubble, and nothing is sent.
describe("WEB-U-355 join's password, said by the page", () => {
  it("says a short password and sends nothing", async () => {
    fetch.mockImplementationOnce(() => Promise.resolve(Response.json(invitee)));
    show();
    const join = await screen.findByRole("button", { name: "Join" });
    expect(join.closest("form")).toHaveAttribute("novalidate");
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "short" },
    });
    fireEvent.click(join);
    expect(
      await screen.findByText("Use at least 12 characters."),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
    expect(fetch).toHaveBeenCalledTimes(1);
  });
});
