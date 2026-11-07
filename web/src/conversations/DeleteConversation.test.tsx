import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { api, ApiError } from "../api/client";
import type { Conversation } from "../protocol/generated/api";
import { conversationId } from "../test/frames";

import { DeleteConversation } from "./DeleteConversation";

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
function show(onClose = vi.fn()) {
  const cache = new QueryClient();
  const invalidate = vi.spyOn(cache, "invalidateQueries");
  render(
    <QueryClientProvider client={cache}>
      <DeleteConversation conversation={conversation} onClose={onClose} />
    </QueryClientProvider>,
  );
  return {
    onClose,
    invalidate,
    dialog: screen.getByRole("dialog", { name: "Delete task" }),
  };
}
afterEach(() => vi.restoreAllMocks());

// Deleting a task asks once, naming it (WEB-U-007: keyboard-operable dialogs; a named confirmation).
describe("WEB-U-007 deleting a task", () => {
  it("names the task, keeps it on Keep and deletes it on Delete", async () => {
    const remove = vi
      .spyOn(api, "deleteConversation")
      .mockResolvedValue(undefined);
    const { onClose, invalidate, dialog } = show();
    // WEB-U-256 (the 14b page pass, S4): deleting is for good, and the workspace's files stay (PRD §12 item 18).
    expect(dialog).toHaveTextContent(
      "Delete “Workspace review”? This stops its environment and deletes the task for good: it can't be undone. The files in its workspace stay.",
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Keep task" }));
    expect(onClose).toHaveBeenCalledOnce();
    expect(remove).not.toHaveBeenCalled();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Delete task" }),
    );
    await vi.waitFor(() => expect(onClose).toHaveBeenCalledTimes(2));
    // A task that leads no team is deleted alone (ADR-0034 S6: with_helpers only for a lead with helpers).
    expect(remove).toHaveBeenCalledWith(
      conversationId,
      expect.any(AbortSignal),
      false,
    );
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["conversations"] });
  });

  it("cannot be dismissed while the deletion is under way", () => {
    vi.spyOn(api, "deleteConversation").mockReturnValue(
      new Promise(() => undefined),
    );
    const { onClose, dialog } = show();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Delete task" }),
    );
    expect(
      within(dialog).getByRole("button", { name: "Keep task" }),
    ).toBeDisabled();
    fireEvent(dialog, new Event("cancel"));
    expect(onClose).not.toHaveBeenCalled();
  });
});

// WEB-U-256: a delete that fails keeps the task, says why in the dialog, and lets the person try again; it never
// closes as if it had worked.
describe("WEB-U-256 a delete that fails", () => {
  it("stays open with the error, and the task stays, when the server can't be reached", async () => {
    vi.spyOn(api, "deleteConversation").mockRejectedValue(
      new ApiError(
        0,
        "network",
        "The server could not be reached. Please try again.",
      ),
    );
    const { onClose, dialog } = show();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Delete task" }),
    );
    expect(
      await within(dialog).findByText(
        "The server could not be reached. Please try again.",
      ),
    ).toBeVisible();
    expect(onClose).not.toHaveBeenCalled();
    expect(
      within(dialog).getByRole("button", { name: "Delete task" }),
    ).toBeEnabled();
  });
});
