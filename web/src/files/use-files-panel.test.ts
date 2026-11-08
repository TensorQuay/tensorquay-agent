import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { useFilesPanel } from "./use-files-panel";

// A finished turn's Open shows a file in the files panel, on its Files tab, as a new request each time.
describe("WEB-U-022 the files panel opened on a file", () => {
  it("opens the panel on the Files tab with the file as a new request each time", () => {
    const { result } = renderHook(() => useFilesPanel(false));
    act(() => result.current.show("changes"));
    expect(result.current.tab).toBe("changes");
    act(() => result.current.showFile("notes/brief.md"));
    expect(result.current.open).toBe(true);
    expect(result.current.tab).toBe("files");
    expect(result.current.target).toEqual({
      path: "notes/brief.md",
      request: 1,
    });
    act(() => result.current.showFile("notes/brief.md"));
    expect(result.current.target?.request).toBe(2);
  });
});

// WEB-U-083: whoever asks, the panel opens a workspace path; a path outside the workspace is marked, never opened.
describe("WEB-U-083 the files panel reads a path as a workspace path", () => {
  it("opens /workspace/… as the workspace's own file", () => {
    const { result } = renderHook(() => useFilesPanel(false));
    act(() => result.current.showFile("/workspace//notes/brief.md"));
    expect(result.current.target).toEqual({
      path: "notes/brief.md",
      request: 1,
    });
  });

  it("marks a path outside the workspace instead of opening it", () => {
    const { result } = renderHook(() => useFilesPanel(false));
    act(() => result.current.showFile("/etc/motd"));
    expect(result.current.open).toBe(true);
    expect(result.current.target).toEqual({
      path: "/etc/motd",
      request: 1,
      outside: true,
    });
  });
});
