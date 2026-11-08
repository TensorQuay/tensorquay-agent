import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Conversation } from "../protocol/generated/api";
import { conversationId } from "../test/frames";
import { templates } from "../test/templates";
import { LiveConversation } from "../transcript/LiveConversation";
import { emptyTranscript } from "../transcript/reducer";
import { useTranscript } from "../transcript/use-transcript";

import { PANEL_STORAGE_KEY } from "./use-files-panel";

vi.mock("../transcript/use-transcript", () => ({ useTranscript: vi.fn() }));
const conversation: Conversation = {
  id: conversationId,
  owner_id: conversationId,
  title: "Workspace review",
  model_route: "local",
  template: "workstation",
  pinned: false,
  title_source: "message",
  backend: "docker",
  state: "active",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  last_activity_at: "2026-01-01T00:00:00Z",
  workspace_id: conversationId,
  mcp_servers: [],
  connectors: [],
  team: null,
};
beforeEach(() => {
  vi.stubGlobal(
    "fetch",
    vi.fn(() =>
      Promise.resolve(Response.json({ path: "", items: [], next_after: null })),
    ),
  );
  vi.mocked(useTranscript).mockImplementation(() => ({
    transcript: { ...emptyTranscript(), sandbox: "ready" },
    connection: "live",
  }));
  localStorage.clear();
});
afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
});
/**
 * A task layout wide enough for the panel to dock beside the conversation, or not (WEB-U-170: the
 * layout's own width decides, not the window's): narrower, the panel lies over the column and starts closed.
 */
function wide(docks: boolean, width = docks ? 1176 : 716) {
  // The layout is measured where the browser observes sizes, as every supported one does.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      const measured = this.classList.contains("conversation-layout")
        ? width
        : 0;
      return {
        width: measured,
        top: 0,
        bottom: 0,
        left: 0,
        right: measured,
      } as DOMRect;
    },
  );
}
function show() {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["templates"], templates);
  cache.setQueryData(["models"], { items: [] });
  return render(
    <QueryClientProvider client={cache}>
      <LiveConversation conversation={conversation} />
    </QueryClientProvider>,
  );
}
const toggle = () => screen.getByRole("button", { name: "Files" });
const panel = () => screen.queryByRole("complementary", { name: "Files" });
/** The panel's own controls, once its code has arrived (WEB-U-176: it loads when wanted). */
const loaded = () => screen.findByRole("button", { name: "Close panel" });

describe("WEB-U-022 files panel placement", () => {
  // The UI standard's header: a quiet Files icon beside the ⋯ (it was the word, the design direction's P2). It is
  // named Files, says so in its tooltip on hover, and opens a panel, not a menu.
  it("is a quiet Files icon, named Files, with its word in a tooltip", () => {
    wide(false);
    show();
    expect(toggle()).not.toHaveTextContent("Files");
    expect(toggle()).not.toHaveAttribute("aria-haspopup");
    fireEvent.pointerEnter(toggle().parentElement as HTMLElement);
    expect(document.querySelector(".action-tooltip")?.textContent).toBe(
      "Files",
    );
  });

  it("starts closed below 1280 px, opens from the bar and returns focus when closed", async () => {
    wide(false);
    show();
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    expect(panel()).toBeNull();
    fireEvent.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(toggle()).toHaveAttribute("aria-controls", panel()?.id);
    await loaded();
    fireEvent.click(
      within(panel() as HTMLElement).getByRole("button", {
        name: "Close panel",
      }),
    );
    expect(panel()).toBeNull();
    expect(toggle()).toHaveFocus();
    expect(localStorage.getItem(PANEL_STORAGE_KEY)).toBeNull();
  });

  // TEST: WEB-U-124
  // WEB-U-124 (found before 17.0): the panel never opens by itself; on a wide screen the person's own choice is kept.
  it("starts closed on wide screens, and keeps the person's choice there", () => {
    wide(true);
    const first = show();
    expect(panel()).toBeNull();
    fireEvent.click(toggle());
    expect(panel()).toBeVisible();
    expect(localStorage.getItem(PANEL_STORAGE_KEY)).toBe("open");
    first.unmount();
    const second = show();
    expect(panel()).toBeVisible();
    fireEvent.click(toggle());
    expect(panel()).toBeNull();
    expect(localStorage.getItem(PANEL_STORAGE_KEY)).toBe("closed");
    second.unmount();
    show();
    expect(panel()).toBeNull();
  });

  // At 1024 with the rail the layout is 920 px and the panel docks there, so the choice is kept there too.
  it("WEB-U-170 keeps the choice wherever the panel docks, 1024 with the rail included", () => {
    wide(true, 920);
    const first = show();
    fireEvent.click(toggle());
    expect(localStorage.getItem(PANEL_STORAGE_KEY)).toBe("open");
    first.unmount();
    show();
    expect(panel()).toBeVisible();
  });

  it("starts closed on narrow screens even when it was left open on a wide one", () => {
    localStorage.setItem(PANEL_STORAGE_KEY, "open");
    wide(false);
    show();
    expect(panel()).toBeNull();
  });

  it("keeps the chosen tab when the panel closes and opens again", async () => {
    wide(false);
    show();
    fireEvent.click(toggle());
    await loaded();
    fireEvent.click(screen.getByRole("tab", { name: "Changes" }));
    fireEvent.click(screen.getByRole("button", { name: "Close panel" }));
    fireEvent.click(toggle());
    expect(screen.getByRole("tab", { name: "Changes" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("offers attachments in the task's composer", () => {
    wide(false);
    show();
    expect(screen.getByRole("button", { name: "Attach files" })).toBeVisible();
  });
});
