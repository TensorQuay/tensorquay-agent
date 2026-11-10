import { describe, expect, it } from "vitest";

import { workspacePath } from "./workspace-path";

// WEB-U-083 (found before 17.0): an agent may name a file by its absolute path in the sandbox, as
// /workspace/index.html; the page opens the workspace's own index.html, never "Workspace / / workspace / index.html".
describe("WEB-U-083 a path from a tool call is a workspace path", () => {
  it.each([
    ["/workspace/index.html", "index.html"],
    ["./index.html", "index.html"],
    ["index.html", "index.html"],
    ["/workspace//a/b", "a/b"],
    ["a//b/./c.txt", "a/b/c.txt"],
    ["/workspace/a/../b.txt", "b.txt"],
    ["  /workspace/report.md  ", "report.md"],
  ])("reads %j as %j", (raw, path) => {
    expect(workspacePath(raw)).toBe(path);
  });

  it.each([
    ["/etc/x", "another folder of the sandbox"],
    ["/workspaces/x", "a folder that only starts like it"],
    ["../x", "a step above the workspace"],
    ["/workspace/../etc/x", "a step above it after the root"],
    ["/workspace", "the workspace itself, not a file"],
    ["", "nothing"],
  ])("refuses %j (%s)", (raw) => {
    expect(workspacePath(raw)).toBeNull();
  });
});
