import { readFileSync } from "node:fs";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { conversationId } from "../test/frames";

import { LazyFilesPanel } from "./LazyFilesPanel";
import { FILES_PANEL_ID } from "./use-files-panel";

const source = (path: string) =>
  readFileSync(new URL(path, import.meta.url), "utf8");

// The conversation route's budget (WEB-U-176): the files panel starts closed, so its code isn't part of the task
// page's first load. Nothing on the page imports it statically; it loads when the person is about to open it.
describe("WEB-U-176 the files panel loads when it is wanted", () => {
  it("isn't imported statically by the task page or its button", () => {
    for (const file of [
      "../transcript/LiveConversation.tsx",
      "./FilesToggle.tsx",
      "./use-files-panel.ts",
    ])
      expect(source(file), file).not.toMatch(
        /from "(\.\/|\.\.\/files\/)FilesPanel"/,
      );
    expect(source("./LazyFilesPanel.tsx")).toMatch(
      /import\("\.\/FilesPanel"\)/,
    );
  });

  // No flash: until its code arrives the panel is already there, the same element in the same place, marked busy.
  it("shows the panel's own frame, busy, until its code arrives", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() =>
        Promise.resolve(
          Response.json({ path: "", items: [], next_after: null }),
        ),
      ),
    );
    render(
      <QueryClientProvider client={new QueryClient()}>
        <LazyFilesPanel
          conversationId={conversationId}
          target={null}
          running={false}
          version="v"
          context={{ kind: "none" }}
          runs={[]}
          tab="files"
          onTab={() => undefined}
          onClose={() => undefined}
        />
      </QueryClientProvider>,
    );
    const frame = screen.getByRole("complementary", { name: "Files" });
    expect(frame).toHaveAttribute("id", FILES_PANEL_ID);
    expect(frame).toHaveClass("files-panel");
    expect(frame).toHaveAttribute("aria-busy", "true");
    expect(
      await screen.findByRole("button", { name: "Close panel" }),
    ).toBeVisible();
    expect(
      screen.getByRole("complementary", { name: "Files" }),
    ).not.toHaveAttribute("aria-busy");
    vi.unstubAllGlobals();
  });
});
