import { readdirSync, readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import zh from "../i18n/locales/zh-Hans.json";
import { catalogue, translator } from "../i18n/translate";
import { GB, sharedWorkspace, workspace } from "../test/workspaces";

import {
  deletePlan,
  leaseState,
  matches,
  meterLevel,
  pickable,
  recentFirst,
  workspaceName,
} from "./workspace-state";

// The rules behind the workspace card, the picker and the delete confirmation.
describe("WEB-U-025 workspace state", () => {
  it("warns at 80 % of the quota and turns danger at 95 %; no meter without a quota or a size", () => {
    expect(meterLevel(7.9 * GB, 10 * GB)).toBe("ok");
    expect(meterLevel(8 * GB, 10 * GB)).toBe("warn");
    expect(meterLevel(9.49 * GB, 10 * GB)).toBe("warn");
    expect(meterLevel(9.5 * GB, 10 * GB)).toBe("danger");
    expect(meterLevel(12 * GB, 10 * GB)).toBe("danger");
    expect(meterLevel(1, null)).toBeNull();
    expect(meterLevel(null, 10 * GB)).toBeNull();
  });

  it("reads the lease as idle, in use or running", () => {
    expect(leaseState(workspace())).toBe("idle");
    expect(
      leaseState(
        workspace({
          lease: {
            conversation_id: "0195d8c0-0000-7000-8000-0000000000c9",
            title: null,
            running: false,
            pinned: false,
          },
        }),
      ),
    ).toBe("held");
    expect(
      leaseState(
        workspace({
          lease: {
            conversation_id: "0195d8c0-0000-7000-8000-0000000000c9",
            title: "Mine",
            running: true,
            pinned: false,
          },
        }),
      ),
    ).toBe("running");
  });

  it("plans a delete from the lease, the task count and the reader's own tasks", () => {
    const empty = workspace();
    expect(deletePlan(empty, 0)).toEqual({ kind: "empty" });
    const mine = workspace({ task_count: 2 });
    expect(deletePlan(mine, 2)).toEqual({ kind: "withTasks", count: 2 });
    // Not all tasks loaded: the gateway still refuses other people's tasks, so the count stands.
    expect(deletePlan(mine, null)).toEqual({ kind: "withTasks", count: 2 });
    expect(deletePlan(workspace({ task_count: 3 }), 1)).toEqual({
      kind: "blocked",
      reason: "othersTasks",
    });
    // Only a going run blocks it; a kept, idle environment is retired by the delete (a UI review before 17.0).
    const held = (running: boolean) =>
      workspace({
        task_count: 1,
        lease: {
          conversation_id: "0195d8c0-0000-7000-8000-0000000000c9",
          title: "Mine",
          running,
          pinned: false,
        },
      });
    expect(deletePlan(held(true), 1)).toEqual({
      kind: "blocked",
      reason: "working",
    });
    expect(deletePlan(held(false), 1)).toEqual({
      kind: "withTasks",
      count: 1,
    });
    // A pinned task keeps its environment on (FR-33): the delete doesn't retire it.
    expect(
      deletePlan(
        workspace({
          task_count: 1,
          lease: {
            conversation_id: "0195d8c0-0000-7000-8000-0000000000c9",
            title: "Mine",
            running: false,
            pinned: true,
          },
        }),
        1,
      ),
    ).toEqual({ kind: "blocked", reason: "pinned" });
    expect(deletePlan(workspace({ state: "forking" }), 0)).toEqual({
      kind: "blocked",
      reason: "notReady",
    });
  });

  it("offers a workspace for a new task only when it is ready and writable", () => {
    expect(pickable(workspace())).toBe("ok");
    expect(pickable(sharedWorkspace("write"))).toBe("ok");
    expect(pickable(sharedWorkspace("read"))).toBe("viewOnly");
    expect(pickable(workspace({ state: "forking" }))).toBe("copying");
    expect(pickable(workspace({ state: "failed" }))).toBe("failed");
  });

  it("orders by last use, then by creation, newest first", () => {
    const old = workspace({ created_at: "2026-01-01T00:00:00Z" });
    const created = workspace({ created_at: "2026-01-03T00:00:00Z" });
    const used = workspace({
      created_at: "2025-12-01T00:00:00Z",
      last_used_at: "2026-01-02T00:00:00Z",
    });
    expect([old, created, used].sort(recentFirst)).toEqual([
      created,
      used,
      old,
    ]);
  });
});

// A pass over the pages before 17.0: a helper's own copy (kind helper_copy, named by its task's title) says so in the
// reader's words, renamed or not, and searches as shown; a person's own workspace shows its name as it is, even one
// that ends in "(helper copy)".
describe("WEB-U-025 a helper's copy is named in the reader's words", () => {
  const chinese = translator("zh-Hans", catalogue(zh));
  const copy = workspace({ name: "researcher", kind: "helper_copy" });

  it("words a helper's copy, in English and in Chinese", () => {
    expect(workspaceName(copy)).toBe("researcher (helper copy)");
    expect(workspaceName(copy, chinese)).toBe("researcher（助手副本）");
    expect(matches(copy, "助手副本", chinese)).toBe(true);
    expect(matches(copy, "researcher", chinese)).toBe(true);
  });

  // WEB-U-260: every catalogue words it, so no language falls back to the English "(helper copy)".
  it("words a helper's copy in every language, never with the English suffix", () => {
    const folder = "src/i18n/locales";
    const languages = readdirSync(folder).filter((file) =>
      file.endsWith(".json"),
    );
    expect(languages.length).toBeGreaterThanOrEqual(10);
    for (const file of languages) {
      const code = file.replace(/\.json$/, "");
      const parsed = JSON.parse(readFileSync(`${folder}/${file}`, "utf8")) as {
        messages: Record<string, string>;
      };
      expect(parsed.messages["workspaces.helperCopyName"], file).toMatch(
        /\{name\}/,
      );
      const named = translator(code, catalogue(parsed))(
        "workspaces.helperCopyName",
        { name: "researcher" },
      );
      expect(named, file).toContain("researcher");
      if (code !== "en") expect(named, file).not.toMatch(/helper copy/);
    }
  });

  it("shows a standard workspace's name as it is", () => {
    for (const name of ["Q3 report", "notes (helper copy)"])
      expect(
        workspaceName(workspace({ name, kind: "standard" }), chinese),
      ).toBe(name);
    const nameless = workspace({ kind: "helper_copy" });
    delete nameless.name;
    expect(workspaceName(nameless, chinese)).toBe("");
  });
});
