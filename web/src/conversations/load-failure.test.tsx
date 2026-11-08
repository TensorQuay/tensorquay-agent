import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { LIMITS } from "../test/user";

import { ConversationPage } from "./ConversationPage";

const id = "0195d8c0-0000-7000-8000-0000000000aa";
const fetch = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  fetch.mockReset();
  fetch.mockImplementation((input) => {
    const url = typeof input === "string" ? input : (input as Request).url;
    if (url === "/api/v1/auth/me")
      return Promise.resolve(
        Response.json({
          id: "0195d8c0-0000-7000-8000-000000000001",
          email: "member@example.invalid",
          role: "member",
          internet: "none",
          limits: LIMITS,
          features: { agent_teams: false },
        }),
      );
    return Promise.resolve(
      Response.json(
        { code: "internal", message: "x", retryable: true },
        { status: 500 },
      ),
    );
  });
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => vi.unstubAllGlobals());

// A task that can't load kept only a red line and a full-width bar. The header stays, and the transcript
// area says one sentence with a normal-size Try again.
describe("WEB-U-017 a task that could not load", () => {
  // A pass over the pages before 17.0: a member opening another's task through a shared workspace is refused 403
  // forbidden. It reads as a 404 does, so the page never hints that the task exists, and offers no Try again that can't
  // help.
  it.each([
    [404, "not_found"],
    [403, "forbidden"],
  ] as const)(
    "says a task that doesn't exist, or can't be opened, in words (%i)",
    async (status, code) => {
      fetch.mockImplementation((input) => {
        const url = typeof input === "string" ? input : (input as Request).url;
        return Promise.resolve(
          url === "/api/v1/auth/me"
            ? Response.json({
                id: "0195d8c0-0000-7000-8000-000000000001",
                email: "member@example.invalid",
                role: "member",
                internet: "none",
                limits: LIMITS,
                features: { agent_teams: false },
              })
            : Response.json(
                { code, message: "x", retryable: false },
                { status },
              ),
        );
      });
      render(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ConversationPage id={id} />
        </QueryClientProvider>,
      );
      expect(await screen.findByRole("alert")).toHaveTextContent(
        "This task doesn't exist, or you can't open it.",
      );
      expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
      // Not a dead end.
      expect(screen.getByRole("link", { name: "New task" })).toHaveAttribute(
        "href",
        "/",
      );
      expect(
        screen.getByRole("button", { name: "Go to your tasks" }),
      ).toBeVisible();
    },
  );

  it("keeps the task header and says so in the transcript's place, with Try again", async () => {
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ConversationPage id={id} />
      </QueryClientProvider>,
    );
    const failure = await screen.findByRole("alert");
    const bar = screen.getByRole("region", { name: "Task header" });
    expect(within(bar).getByRole("heading", { level: 1 })).toHaveTextContent(
      "Task",
    );
    expect(failure.closest(".conversation-failure")).not.toBeNull();
    const retry = screen.getByRole("button", { name: "Try again" });
    expect(retry.closest(".conversation-failure")).not.toBeNull();
    const calls = fetch.mock.calls.length;
    fireEvent.click(retry);
    await vi.waitFor(() =>
      expect(fetch.mock.calls.length).toBeGreaterThan(calls),
    );
  });
});
