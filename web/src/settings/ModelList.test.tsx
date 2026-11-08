import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import type { RouteModels } from "../protocol/generated/api";

import type { RoutesApi } from "./admin/routes";
import { ModelList } from "./ModelList";

const listing = (extra: Partial<RouteModels> = {}): RouteModels => ({
  route: "deepseek",
  protocol: "openai_chat",
  models: [
    { id: "deepseek-chat", display_name: "DeepSeek Chat" },
    { id: "deepseek-reasoner" },
  ],
  truncated: false,
  failure: null,
  ...extra,
});

/** Eight models or more: enough to filter. */
const many = (): RouteModels =>
  listing({
    models: [
      { id: "deepseek-chat", display_name: "DeepSeek Chat" },
      { id: "deepseek-reasoner" },
      ...Array.from({ length: 6 }, (_, index) => ({
        id: `other-model-${index + 1}`,
      })),
    ],
  });

function show({
  answer = Promise.resolve(listing()),
  route = "deepseek",
  stale = false,
  value = "deepseek-chat",
}: {
  answer?: Promise<RouteModels>;
  route?: string | null;
  stale?: boolean;
  value?: string;
} = {}) {
  const models = vi.fn<RoutesApi["models"]>(() => answer);
  const pick = vi.fn<(id: string) => void>();
  render(
    <ModelList
      api={{ models } as unknown as RoutesApi}
      route={route}
      stale={stale}
      value={value}
      onPick={pick}
    />,
  );
  return { models, pick };
}
const list = () => screen.getByRole("button", { name: "List models" });

// Lists what the saved route's server offers, to pick Model from (WEB-U-067): only for a saved route, only while its
// address and key are the saved ones; a refusal or an unusable answer is named, never a guess.
describe("WEB-U-067 List models in the route form", () => {
  // A short list is a few buttons: nothing to filter, and each is reached with Tab.
  it("shows a short list (fewer than eight) as buttons without a filter, and picks by click or keyboard", async () => {
    const { models, pick } = show();
    fireEvent.click(list());
    expect(models).toHaveBeenCalledWith("deepseek", expect.anything());
    const box = await screen.findByRole("list", { name: "Models" });
    expect(screen.getByText("2 models")).toBeVisible();
    expect(screen.queryByLabelText("Filter models")).toBeNull();
    expect(screen.queryByRole("listbox")).toBeNull();
    const current = within(box).getByRole("button", { name: /deepseek-chat/ });
    // The current model says so in words (UI-GUIDELINES §9), and takes the focus first.
    expect(current).toHaveTextContent("deepseek-chatDeepSeek ChatCurrent");
    expect(current).toHaveFocus();
    fireEvent.click(
      within(box).getByRole("button", { name: "deepseek-reasoner" }),
    );
    expect(pick).toHaveBeenCalledWith("deepseek-reasoner");
    expect(screen.queryByRole("list", { name: "Models" })).toBeNull();
    fireEvent.click(list());
    fireEvent.keyDown(
      await screen.findByRole("button", { name: /deepseek-chat/ }),
      { key: "Escape" },
    );
    expect(screen.queryByRole("list", { name: "Models" })).toBeNull();
  });

  it("lists the saved route's models, filters them and fills Model with the pick", async () => {
    const { models, pick } = show({ answer: Promise.resolve(many()) });
    fireEvent.click(list());
    expect(models).toHaveBeenCalledWith("deepseek", expect.anything());
    const box = await screen.findByRole("listbox", { name: "Models" });
    expect(screen.getByText("8 models")).toBeVisible();
    expect(
      within(box)
        .getAllByRole("option")
        .slice(0, 2)
        .map((option) => option.textContent),
    ).toEqual(["deepseek-chatDeepSeek ChatCurrent", "deepseek-reasoner"]);
    const current = within(box).getByRole("option", { name: /deepseek-chat/ });
    expect(current).toHaveAttribute("aria-selected", "true");
    // Not by colour alone (UI-GUIDELINES §9): the current model says so in words.
    expect(within(current).getByText("Current")).toBeVisible();
    fireEvent.change(screen.getByLabelText("Filter models"), {
      target: { value: "reason" },
    });
    fireEvent.click(
      within(box).getByRole("option", { name: "deepseek-reasoner" }),
    );
    expect(pick).toHaveBeenCalledWith("deepseek-reasoner");
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("picks with the keyboard, and closes on Escape", async () => {
    const { pick } = show({ value: "", answer: Promise.resolve(many()) });
    fireEvent.click(list());
    const filter = await screen.findByLabelText("Filter models");
    expect(filter).toHaveFocus();
    fireEvent.keyDown(filter, { key: "ArrowDown" });
    fireEvent.keyDown(filter, { key: "ArrowDown" });
    fireEvent.keyDown(filter, { key: "Enter" });
    expect(pick).toHaveBeenCalledWith("deepseek-reasoner");
    fireEvent.click(list());
    fireEvent.keyDown(await screen.findByLabelText("Filter models"), {
      key: "Escape",
    });
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("says when nothing matches the filter", async () => {
    show({ answer: Promise.resolve(many()) });
    fireEvent.click(list());
    fireEvent.change(await screen.findByLabelText("Filter models"), {
      target: { value: "gpt" },
    });
    expect(screen.getByText("No model matches.")).toBeVisible();
  });

  it("names an empty list", async () => {
    show({ answer: Promise.resolve(listing({ models: [] })) });
    fireEvent.click(list());
    expect(
      await screen.findByText(
        "The server listed no models. Type the name in Model.",
      ),
    ).toBeVisible();
  });

  it("names a truncated list", async () => {
    show({ answer: Promise.resolve(listing({ truncated: true })) });
    fireEvent.click(list());
    expect(
      await screen.findByText(
        "The server offers more than 500 models; these are the first 500. Type the name in Model if yours isn't here.",
      ),
    ).toBeVisible();
  });

  it("is only for a saved route", () => {
    show({ route: null });
    expect(screen.queryByRole("button", { name: "List models" })).toBeNull();
  });

  it("is disabled with unsaved address or key changes, and says why", () => {
    show({ stale: true });
    expect(list()).toBeDisabled();
    expect(
      screen.getByText(
        "Save the route first: the list uses its saved address and key.",
      ),
    ).toBeVisible();
  });

  it("names the server's refusal with what it answered", async () => {
    show({
      answer: Promise.resolve(
        listing({
          models: [],
          failure: { code: "auth_rejected", status: 401, excerpt: "bad key" },
        }),
      ),
    });
    fireEvent.click(list());
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The server refused the route’s key.",
    );
    expect(screen.getByText("What the server answered")).toBeVisible();
    expect(screen.getByText("Status 401")).toBeVisible();
    expect(screen.getByText("bad key")).toBeVisible();
  });

  it("names a missing list and an unreadable one in the listing's own words", async () => {
    show({
      answer: Promise.resolve(
        listing({
          models: [],
          failure: { code: "not_found", status: 404, excerpt: null },
        }),
      ),
    });
    fireEvent.click(list());
    expect(
      await screen.findByText("The server has no model list at this address."),
    ).toBeVisible();
  });

  it("names an unreadable list with what was wrong", async () => {
    show({
      answer: Promise.resolve(
        listing({
          models: [],
          failure: {
            code: "models_unreadable",
            status: 200,
            excerpt: null,
            detail: "data is not an array",
          },
        }),
      ),
    });
    fireEvent.click(list());
    expect(
      await screen.findByText("The server's model list could not be read."),
    ).toBeVisible();
    expect(screen.getByText("data is not an array")).toBeVisible();
  });

  // Exploring hits the 30 s wait; the last list stays useful, so it shows with the wait, not an error in its place.
  it("shows the last list while the gateway asks to wait, even after the form was closed", async () => {
    const models = vi
      .fn<RoutesApi["models"]>()
      .mockResolvedValueOnce(many())
      .mockRejectedValue(new ApiError(429, "rate_limited", "x", 29));
    const api = { models } as unknown as RoutesApi;
    const form = () => (
      <ModelList
        api={api}
        route="deepseek"
        stale={false}
        value="deepseek-chat"
        onPick={vi.fn()}
      />
    );
    const first = render(form());
    fireEvent.click(list());
    await screen.findByRole("listbox", { name: "Models" });
    first.unmount();
    render(form());
    fireEvent.click(list());
    expect(
      await screen.findByText(
        "Showing the last list. You can list again in 29 s.",
      ),
    ).toBeVisible();
    expect(
      within(screen.getByRole("listbox", { name: "Models" }))
        .getAllByRole("option")
        .map((option) => option.getAttribute("id") !== null),
    ).toEqual(Array.from({ length: 8 }, () => true));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it.each([
    [new ApiError(409, "key_missing", "x"), "This route has no key yet."],
    [
      new ApiError(409, "models_running", "x"),
      "A listing of this route is already running.",
    ],
    [
      new ApiError(429, "rate_limited", "x", 12),
      "Listed too recently. Try again in 12 s.",
    ],
    [
      new ApiError(504, "timeout", "x"),
      "The listing took longer than a minute and stopped.",
    ],
    [
      new ApiError(503, "store_unavailable", "x"),
      "The models could not be listed.",
    ],
  ])("names the gateway's refusal %#", async (error, words) => {
    show({ answer: Promise.reject(error) });
    fireEvent.click(list());
    // An alert, so a screen reader hears it without the focus moving (WEB-U-067).
    expect(await screen.findByRole("alert")).toHaveTextContent(words);
  });
});
