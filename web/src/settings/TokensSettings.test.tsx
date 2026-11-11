import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ApiToken } from "../protocol/generated/api";
import { choose, chosen } from "../test/picker";

import { TokensSettings } from "./TokensSettings";

const stamp = "2026-01-01T00:00:00Z";
const ci: ApiToken = {
  id: "0195d8c0-0000-7000-8000-00000000000a",
  name: "CI",
  hint: "wxyz",
  created_at: stamp,
  expires_at: "2026-04-01T00:00:00Z",
  last_used_at: null,
};
const fetch = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  fetch.mockReset();
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => {
  vi.unstubAllGlobals();
});
const json = (value: unknown, status = 200) =>
  Promise.resolve(Response.json(value, { status }));
const problem = (status: number, code: string) =>
  json({ error: { code, message: "diagnostic", retryable: false } }, status);

// A fixed clock: CI's token expires on 1 April 2026, after this day.
const march = () => new Date("2026-03-01T12:00:00Z");
function show(clock: () => Date = march) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <TokensSettings clock={clock} />
    </QueryClientProvider>,
  );
}

// FR-26: personal API tokens - listed without their secret, shown once on creation, revoked with a confirm.
describe("WEB-U-023 API tokens", () => {
  it("explains Bearer use and shows the empty state", async () => {
    fetch.mockImplementationOnce(() => json({ items: [] }));
    show();
    expect(await screen.findByText("No tokens yet.")).toBeInTheDocument();
    expect(
      screen.getByText("Authorization: Bearer <token>"),
    ).toBeInTheDocument();
    expect(chosen("Expires in")).toBe("90 days");
  });

  it("lists tokens by hint, never by secret, with Never for an unused token", async () => {
    fetch.mockImplementationOnce(() => json({ items: [ci] }));
    show();
    const row = (await screen.findByText("CI")).closest("tr");
    if (!row) throw new Error("The token must be a table row");
    expect(row).toHaveTextContent("tqa_pat_…wxyz");
    expect(row).toHaveTextContent("Never");
  });

  it("creates a token, shows it once and refreshes the list", async () => {
    const secret = `tqa_pat_${"s".repeat(39)}wxyz`;
    fetch
      .mockImplementationOnce(() => json({ items: [] }))
      .mockImplementationOnce(() => json({ ...ci, token: secret }, 201))
      .mockImplementationOnce(() => json({ items: [ci] }));
    show();
    await screen.findByText("No tokens yet.");
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), {
      target: { value: " CI " },
    });
    choose("Expires in", "30 days");
    fireEvent.click(screen.getByRole("button", { name: "Create token" }));
    const dialog = await screen.findByRole("dialog", { name: "Token created" });
    expect(within(dialog).getByRole("textbox", { name: "Token" })).toHaveValue(
      secret,
    );
    expect(dialog).toHaveTextContent("You won’t be able to see it again.");
    expect(fetch.mock.calls[1]?.[1]?.body).toBe(
      JSON.stringify({ name: "CI", expires_in_days: 30 }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "I’ve copied it" }),
    );
    expect(await screen.findByText("CI")).toBeInTheDocument();
    expect(screen.queryByDisplayValue(secret)).not.toBeInTheDocument();
  });

  it("says plainly when the name is taken", async () => {
    fetch
      .mockImplementationOnce(() => json({ items: [ci] }))
      .mockImplementationOnce(() => problem(409, "token_name_taken"));
    show();
    await screen.findByText("CI");
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), {
      target: { value: "CI" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create token" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "You already have a token with this name.",
    );
  });

  // WEB-U-191: Revoke is a danger item under the row's ⋯, named for the token, as in every settings table: never a bare
  // destructive button in a row. It asks first, and only then sends.
  it("revokes from the row's ⋯ after a named confirmation", async () => {
    fetch
      .mockImplementationOnce(() => json({ items: [ci] }))
      .mockImplementationOnce(() =>
        Promise.resolve(new Response(null, { status: 204 })),
      )
      .mockImplementationOnce(() => json({ items: [] }));
    show();
    const more = await screen.findByRole("button", {
      name: "More actions for the token “CI”",
    });
    expect(screen.queryByRole("button", { name: /^Revoke/ })).toBeNull();
    fireEvent.click(more);
    const revoke = screen.getByRole("menuitem", { name: "Revoke token “CI”" });
    expect(revoke).toHaveTextContent("Revoke…");
    expect(revoke).toHaveAttribute("data-danger");
    fireEvent.click(revoke);
    const dialog = screen.getByRole("dialog", { name: "Revoke this token?" });
    expect(dialog).toHaveTextContent(
      "Programs using “CI” stop working at once.",
    );
    expect(fetch).toHaveBeenCalledTimes(1);
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Revoke token" }),
    );
    expect(await screen.findByText("No tokens yet.")).toBeInTheDocument();
    expect(fetch.mock.calls[1]?.[0]).toBe(`/api/v1/tokens/${ci.id}`);
    // As after an invitation's revoke: the row and its ⋯ are gone, and focus is the page's.
    expect(document.activeElement).toBe(document.body);
  });

  it("gives focus back to the row's ⋯ when the revoke is cancelled, and sends nothing", async () => {
    fetch.mockImplementationOnce(() => json({ items: [ci] }));
    show();
    const more = await screen.findByRole("button", {
      name: "More actions for the token “CI”",
    });
    fireEvent.click(more);
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Revoke token “CI”" }),
    );
    fireEvent.click(
      within(
        screen.getByRole("dialog", { name: "Revoke this token?" }),
      ).getByRole("button", { name: "Cancel" }),
    );
    expect(more).toHaveFocus();
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("marks an expired token and greys its row", async () => {
    fetch.mockImplementationOnce(() => json({ items: [ci] }));
    show(() => new Date("2026-09-29T08:00:00Z"));
    const row = (await screen.findByText("CI")).closest("tr");
    if (!row) throw new Error("The token must be a table row");
    expect(within(row).getByText("Expired")).toBeVisible();
    expect(row).toHaveAttribute("data-expired", "true");
  });

  it("shows no Expired mark before the token expires", async () => {
    fetch.mockImplementationOnce(() => json({ items: [ci] }));
    show();
    const row = (await screen.findByText("CI")).closest("tr");
    if (!row) throw new Error("The token must be a table row");
    expect(within(row).queryByText("Expired")).toBeNull();
    expect(row).not.toHaveAttribute("data-expired");
  });

  it("offers a retry when the list cannot load", async () => {
    fetch
      .mockImplementationOnce(() => problem(503, "store_unavailable"))
      .mockImplementationOnce(() => json({ items: [ci] }));
    show();
    expect(
      await screen.findByText("Your tokens could not be loaded."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("CI")).toBeInTheDocument();
  });
});

// WEB-U-355 (a pass over the pages before 17.0): an empty name is said under the field in the app's words, never by the
// browser's own bubble (UI-GUIDELINES §5, input errors), and nothing is sent.
describe("WEB-U-355 the token's name, said by the page", () => {
  it("says an empty name under the field and sends nothing", async () => {
    fetch.mockImplementation(() => json({ items: [] }));
    show();
    const create = await screen.findByRole("button", { name: "Create token" });
    expect(create.closest("form")).toHaveAttribute("novalidate");
    fireEvent.click(create);
    const name = screen.getByRole("textbox", { name: "Name" });
    expect(
      await screen.findByText("Give the token a name."),
    ).toBeInTheDocument();
    expect(name).toHaveAttribute("aria-invalid", "true");
    expect(
      fetch.mock.calls.filter(([, init]) => init?.method === "POST"),
    ).toEqual([]);
    fireEvent.change(name, { target: { value: "CI" } });
    expect(screen.queryByText("Give the token a name.")).toBeNull();
  });
});
