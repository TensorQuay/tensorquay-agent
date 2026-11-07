import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { conversationId, runId } from "../test/frames";
import { openTaskChoices } from "../test/start-choices";
import { templates } from "../test/templates";
import { shellData } from "../test/workspaces";
import { emptyTranscript } from "../transcript/reducer";

import { ConversationControls } from "./ConversationControls";
import { TaskChoices } from "./TaskChoices";

const conversation = {
  id: conversationId,
  owner_id: conversationId,
  title: "Environment review",
  model_route: "local",
  template: "minimal",
  pinned: false,
  title_source: "message" as const,
  backend: "docker",
  state: "active",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  last_activity_at: "2026-01-01T00:00:00Z",
  workspace_id: "0195d8c0-0000-7000-8000-0000000000ff",
  mcp_servers: [] as string[],
  connectors: [] as string[],
  team: null,
} as const;
const fetch = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  fetch.mockReset();
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** Opens the task's model and environment panel from its header, then finds a picker in it by name. */
async function inHeader(name: string) {
  await openTaskChoices();
  return screen.findByRole("button", { name });
}

function page(working = false) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["templates"], templates);
  cache.setQueryData(["models"], { items: [] });
  shellData(cache);
  const element = (busy: boolean) => (
    <QueryClientProvider client={cache}>
      <>
        <ConversationControls
          conversation={conversation}
          transcript={{
            ...emptyTranscript(),
            sandbox: "ready",
            run: { id: busy ? runId : null, status: busy ? "running" : "idle" },
          }}
        />
        <TaskChoices
          conversation={conversation}
          transcript={{
            ...emptyTranscript(),
            sandbox: "ready",
            run: { id: busy ? runId : null, status: busy ? "running" : "idle" },
          }}
        />
      </>
    </QueryClientProvider>
  );
  const view = render(element(working));
  return { cache, working: () => view.rerender(element(true)) };
}
async function choose() {
  fireEvent.click(await inHeader("Environment: Minimal"));
  fireEvent.click(screen.getByRole("menuitemradio", { name: /^Workstation/ }));
  await screen.findByRole("group", { name: "Switch environment" });
}
function confirm() {
  fireEvent.click(
    screen.getByRole("button", { name: "Switch to Workstation" }),
  );
}

describe("WEB-U-018 persisted environment selection", () => {
  it("waits for inline confirmation and a 200 before showing the new environment, without inventing a transcript event", async () => {
    let finish: ((response: Response) => void) | undefined;
    fetch.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const { cache } = page();
    await choose();
    expect(fetch).not.toHaveBeenCalled();
    expect(
      screen.getByRole("group", { name: "Switch environment" }),
    ).toHaveTextContent("Files and history stay; anything still running stops");
    confirm();
    await waitFor(() => expect(fetch).toHaveBeenCalledOnce());
    expect(await inHeader("Environment: Minimal")).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Cancel switch" }),
    ).toBeDisabled();
    await act(() => {
      finish?.(Response.json({ ...conversation, template: "workstation" }));
      return Promise.resolve();
    });
    await inHeader("Environment: Workstation");
    expect(
      screen.queryByRole("group", { name: "Switch environment" }),
    ).toBeNull();
    expect(screen.queryByText(/Switched to/)).toBeNull();
    expect(cache.getQueryData(["conversation", conversationId])).toMatchObject({
      template: "workstation",
    });
    const [url, options] = fetch.mock.calls[0] ?? [];
    expect(url).toBe(`/api/v1/conversations/${conversationId}/environment`);
    expect(options?.method).toBe("PUT");
    expect(options?.body).toBe(JSON.stringify({ template: "workstation" }));
    expect(new Headers(options?.headers).get("idempotency-key")).toMatch(
      /^[a-f0-9-]{36}$/,
    );
  });

  it("cancels without a request and never changes an already selected template", async () => {
    page();
    await choose();
    fireEvent.click(screen.getByRole("button", { name: "Cancel switch" }));
    expect(
      screen.queryByRole("group", { name: "Switch environment" }),
    ).toBeNull();
    fireEvent.click(await inHeader("Environment: Minimal"));
    fireEvent.click(screen.getByRole("menuitemradio", { name: /^Minimal/ }));
    expect(
      screen.queryByRole("group", { name: "Switch environment" }),
    ).toBeNull();
    expect(fetch).not.toHaveBeenCalled();
  });

  it("reuses the same action key after uncertain delivery", async () => {
    fetch
      .mockRejectedValueOnce(new Error("Lost response"))
      .mockResolvedValueOnce(
        Response.json({ ...conversation, template: "workstation" }),
      );
    page();
    await choose();
    confirm();
    await screen.findByRole("alert");
    expect(await inHeader("Environment: Minimal")).toBeVisible();
    confirm();
    await inHeader("Environment: Workstation");
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(
      new Headers(fetch.mock.calls[0]?.[1]?.headers).get("idempotency-key"),
    ).toBe(
      new Headers(fetch.mock.calls[1]?.[1]?.headers).get("idempotency-key"),
    );
  });

  it.each([
    [
      409,
      "run_active",
      "Stop the current run before changing the environment.",
    ],
    [
      422,
      "unknown_template",
      "This environment is no longer available. Choose another environment.",
    ],
  ] as const)(
    "keeps the current selection on %s and maps the error code",
    async (status, code, message) => {
      fetch.mockResolvedValueOnce(
        Response.json(
          { error: { code, message: "Private diagnostic", retryable: false } },
          { status },
        ),
      );
      page();
      await choose();
      confirm();
      expect(await screen.findByRole("alert")).toHaveTextContent(message);
      expect(await inHeader("Environment: Minimal")).toBeVisible();
      expect(screen.queryByText(/Private diagnostic/)).toBeNull();
    },
  );

  // The pickers are in the task's header (WEB-U-113); choosing an environment asks to switch above the composer, and
  // its menu is closed, so nothing overlaps.
  it("asks to switch above the composer when an environment is chosen, with its menu closed", async () => {
    page();
    await choose();
    expect(screen.queryByRole("menu")).toBeNull();
    expect(
      screen.getByRole("group", { name: "Switch environment" }),
    ).toBeVisible();
    expect(
      screen.getByRole("group", { name: "Model and environment" }),
    ).toBeVisible();
  });

  it("disables the picker with a reason throughout a run", async () => {
    page(true);
    const picker = await inHeader("Environment: Minimal");
    expect(picker).toHaveAttribute("aria-disabled", "true");
    expect(picker).toHaveAccessibleDescription(
      "Stop the current run before changing the environment.",
    );
    fireEvent.click(picker);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(fetch).not.toHaveBeenCalled();
  });

  it("prevents a stale confirmation if a run starts after opening it", async () => {
    const view = page();
    await choose();
    view.working();
    expect(
      screen.getByRole("button", { name: "Switch to Workstation" }),
    ).toBeDisabled();
    confirm();
    expect(fetch).not.toHaveBeenCalled();
  });
});
