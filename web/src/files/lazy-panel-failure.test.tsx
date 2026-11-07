import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { conversationId } from "../test/frames";

import { LazyFilesPanel } from "./LazyFilesPanel";
import { FILES_PANEL_ID } from "./use-files-panel";

/** Whether the panel's code fails, as when its chunk can't be fetched offline. */
const broken = vi.hoisted(() => ({ now: true }));
vi.mock("./FilesPanel", () => ({
  FilesPanel: () => {
    if (broken.now) throw new Error("chunk failed");
    return <p>{"The workspace's files"}</p>;
  },
}));

beforeEach(() => {
  broken.now = true;
  // React reports the caught error; the boundary is what is under test.
  vi.spyOn(console, "error").mockImplementation(() => undefined);
});
afterEach(() => {
  vi.restoreAllMocks();
});

// The 14b page pass, E5: offline, the files panel's error replaced the whole panel, its header too: no name, no close,
// the text flush at its top edge. The failure keeps the panel's frame, its name and its close, and Try again loads it.
describe("WEB-U-253 the files panel keeps its header above an error", () => {
  it("names the panel, closes it, and loads it again", async () => {
    const onClose = vi.fn();
    render(
      <LazyFilesPanel
        conversationId={conversationId}
        target={null}
        running={false}
        version="v"
        context={{ kind: "none" }}
        runs={[]}
        tab="files"
        onTab={() => undefined}
        onClose={onClose}
      />,
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This panel could not be displayed.",
    );
    const frame = screen.getByRole("complementary", { name: "Files" });
    expect(frame).toHaveAttribute("id", FILES_PANEL_ID);
    expect(
      screen.getByRole("heading", { level: 2, name: "Files" }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Close panel" }));
    expect(onClose).toHaveBeenCalledOnce();
    broken.now = false;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("The workspace's files")).toBeVisible();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
