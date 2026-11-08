import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { FileChange } from "../protocol/generated/api";
import { conversationId } from "../test/frames";

import { ChangesView, type RunChoice } from "./ChangesView";

type Answer = { status?: number; body: unknown };
const change = (
  path: string,
  status: FileChange["status"],
  extra: Partial<FileChange> = {},
): FileChange => ({
  path,
  status,
  added: 0,
  removed: 0,
  binary: false,
  too_large: false,
  ...extra,
});
const refusal = (status: number, code: string): Answer => ({
  status,
  body: { error: { code, message: "Refused.", retryable: false } },
});
const page = (
  files: FileChange[],
  extra: { live?: boolean; running?: boolean; truncated?: boolean } = {},
): Answer => ({
  body: {
    run_id: "run",
    live: false,
    running: false,
    truncated: false,
    files,
    ...extra,
  },
});
let runs: Record<string, Answer> = {};
let diffs: Record<string, Answer> = {};
const fetch = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  runs = {
    "run-1": page([change("report.md", "added", { added: 3 })]),
    "run-2": page([
      change("report.md", "modified", { added: 1, removed: 2 }),
      change("data.bin", "modified", { binary: true }),
      change("big.log", "added", { too_large: true }),
      change("old.txt", "deleted", { removed: 4 }),
    ]),
  };
  diffs = {};
  fetch.mockReset().mockImplementation((input) => {
    const url = new URL(input as string, "http://localhost");
    const run = /\/runs\/([^/]+)\/changes/.exec(url.pathname)?.[1] ?? "";
    const answer = url.pathname.endsWith("/diff")
      ? diffs[`${run}:${url.searchParams.get("path")}`]
      : runs[run];
    return Promise.resolve(
      Response.json(answer?.body ?? refusal(404, "not_found").body, {
        status: answer ? (answer.status ?? 200) : 404,
      }),
    );
  });
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => {
  vi.unstubAllGlobals();
});

const choices: RunChoice[] = [
  { id: "run-1", label: "Write the report", startedAt: "2026-01-01T09:00:00Z" },
  { id: "run-2", label: "Tidy the report", startedAt: "2026-01-01T10:00:00Z" },
];
function wrapper() {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return function Providers({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={cache}>{children}</QueryClientProvider>;
  };
}
function show(props: Partial<Parameters<typeof ChangesView>[0]> = {}) {
  return render(
    <ChangesView
      conversationId={conversationId}
      runs={choices}
      running={false}
      {...props}
    />,
    { wrapper: wrapper() },
  );
}
const rows = async () =>
  within(await screen.findByRole("list", { name: "Changed files" }))
    .getAllByRole("listitem")
    .map((item) => item.textContent);

// A file the run created, or an image it changed, opens as what it is; a text file it edited shows its diff.
describe("WEB-U-024 opening a changed file", () => {
  it("opens a created file and a changed image in the files panel, and an edited text file's diff", async () => {
    runs["run-2"] = page([
      change("brief.md", "added", { added: 4 }),
      change("out/chart.png", "modified", { binary: true }),
      change("report.md", "modified", { added: 1, removed: 2 }),
    ]);
    diffs["run-2:report.md"] = {
      body: {
        path: "report.md",
        status: "modified",
        unified: "--- a/report.md\n+++ b/report.md\n@@ -1 +1 @@\n-old\n+new\n",
        binary: false,
        too_large: false,
        truncated: false,
      },
    };
    const onOpenFile = vi.fn();
    show({ onOpenFile });
    expect(await rows()).toEqual([
      "Createdbrief.md+4",
      "Updatedout/chart.pngBinary",
      "Modifiedreport.md+1−2",
    ]);
    const list = screen.getByRole("list", { name: "Changed files" });
    fireEvent.click(within(list).getByRole("button", { name: /brief\.md/ }));
    fireEvent.click(within(list).getByRole("button", { name: /chart\.png/ }));
    expect(onOpenFile.mock.calls).toEqual([["brief.md"], ["out/chart.png"]]);
    expect(
      fetch.mock.calls.filter(([input]) => (input as string).includes("/diff")),
    ).toHaveLength(0);
    fireEvent.click(within(list).getByRole("button", { name: /report\.md/ }));
    await screen.findByRole("region", { name: "report.md" });
    expect(onOpenFile).toHaveBeenCalledTimes(2);
  });
});

describe("WEB-U-024 run changes", () => {
  it("shows the latest run's changes with status, counts and plain notes", async () => {
    show();
    // The app's picker, the whole message, "Latest" as a badge rather than a prefix.
    const picker = screen.getByRole("button", { name: "Run: Tidy the report" });
    expect(within(picker).getByText("Latest")).toBeVisible();
    expect(await rows()).toEqual([
      "Modifiedreport.md+1−2",
      "Modifieddata.binBinary",
      "Createdbig.logToo large to show",
      "Deletedold.txt−4",
    ]);
    expect(
      screen.getByText("Your uploads are not listed as changes."),
    ).toBeVisible();
    // Generated caches are left out quietly (saying so was noise).
    expect(screen.queryByText(/__pycache__/)).toBeNull();
  });

  it("switches to an earlier run", async () => {
    show();
    await rows();
    fireEvent.click(screen.getByRole("button", { name: /^Run: / }));
    fireEvent.click(
      screen.getByRole("menuitemradio", { name: /^Write the report/ }),
    );
    await waitFor(async () =>
      expect(await rows()).toEqual(["Createdreport.md+3"]),
    );
  });

  it("follows a new run even after an earlier one was picked", async () => {
    runs["run-3"] = page([change("notes.txt", "added", { added: 1 })]);
    const { rerender } = show();
    await rows();
    fireEvent.click(screen.getByRole("button", { name: /^Run: / }));
    fireEvent.click(
      screen.getByRole("menuitemradio", { name: /^Write the report/ }),
    );
    await waitFor(async () =>
      expect(await rows()).toEqual(["Createdreport.md+3"]),
    );
    rerender(
      <ChangesView
        conversationId={conversationId}
        runs={[
          ...choices,
          {
            id: "run-3",
            label: "Add notes",
            startedAt: "2026-01-01T11:00:00Z",
          },
        ]}
        running={false}
      />,
    );
    expect(
      screen.getByRole("button", { name: "Run: Add notes" }),
    ).toBeVisible();
    await waitFor(async () =>
      expect(await rows()).toEqual(["Creatednotes.txt+1"]),
    );
  });

  // An open diff belongs to its run. When a new run becomes the latest, the view follows it to its list
  // (the rule for a picked run) instead of asking the new run for the old path.
  it("WEB-U-028 returns from an open diff to the new run's list when a newer run starts", async () => {
    diffs["run-2:report.md"] = {
      body: {
        path: "report.md",
        status: "modified",
        unified: "--- a/report.md\n+++ b/report.md\n@@ -1 +1 @@\n-old\n+new\n",
        binary: false,
        too_large: false,
        truncated: false,
      },
    };
    runs["run-3"] = page([]);
    const { rerender } = show();
    fireEvent.click(await screen.findByRole("button", { name: /report\.md/ }));
    await screen.findByRole("region", { name: "report.md" });
    rerender(
      <ChangesView
        conversationId={conversationId}
        runs={[
          ...choices,
          { id: "run-3", label: "Say ok", startedAt: "2026-01-01T11:00:00Z" },
        ]}
        running={false}
      />,
    );
    expect(await screen.findByText("This run changed no files.")).toBeVisible();
    expect(screen.queryByRole("region", { name: "report.md" })).toBeNull();
    expect(screen.queryByText("This file is no longer there.")).toBeNull();
    const asked = fetch.mock.calls.map(([input]) => input as string);
    expect(
      asked.filter((url) => url.includes("/runs/run-3/changes/diff")),
    ).toEqual([]);
  });

  it("opens one file's diff with added and removed lines marked, and returns to the list", async () => {
    diffs["run-2:report.md"] = {
      body: {
        path: "report.md",
        status: "modified",
        unified:
          "--- a/report.md\n+++ b/report.md\n@@ -1,2 +1 @@\n # Report\n-old line\n-second\n+new line\n",
        binary: false,
        too_large: false,
        truncated: true,
      },
    };
    show();
    fireEvent.click(await screen.findByRole("button", { name: /report\.md/ }));
    const diff = await screen.findByRole("region", { name: "report.md" });
    // The diff says which file it shows, as its list row does.
    const heading = within(diff).getByRole("heading", { level: 2 });
    expect(heading).toHaveTextContent("Modifiedreport.md+1−2");
    expect(within(heading).getByTitle("report.md")).toBeVisible();
    const lines = within(diff).getAllByRole("listitem");
    expect(lines.map((line) => line.dataset.kind)).toEqual([
      "hunk",
      "context",
      "removed",
      "removed",
      "added",
    ]);
    expect(within(diff).queryByText(/^\+\+\+/)).toBeNull();
    expect(
      within(diff).getByText("Showing the first 512 kB of the changes."),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "All changes" }));
    expect(await rows()).toHaveLength(4);
  });

  it.each([
    [page([]), "This run changed no files."],
    [
      page([change("a.txt", "added", { added: 1 })], {
        live: true,
        running: true,
      }),
      "The agent is working; these are the changes so far.",
    ],
    [
      page([change("a.txt", "added", { added: 1 })], { truncated: true }),
      "Some files were not compared because the workspace is very large.",
    ],
    [
      refusal(404, "changes_unavailable"),
      "Changes are recorded from the next message on.",
    ],
    [
      refusal(409, "files_unavailable"),
      "Files are not available in this environment.",
    ],
    [refusal(500, "internal"), "The changes could not be loaded."],
  ])("says plainly what it can show", async (answer, copy) => {
    runs["run-2"] = answer;
    show();
    expect(await screen.findByText(copy)).toBeVisible();
  });

  // A screen review before 17.0: hunk headers were printed raw ("@@ -1,2 +1 @@"). Each is a quiet separator in words,
  // and a changed line says what it is to assistive technology, not only by its tint and its + or −.
  it("WEB-U-164 says where each change is in words, never as @@", async () => {
    diffs["run-2:report.md"] = {
      body: {
        path: "report.md",
        status: "modified",
        unified:
          "--- a/report.md\n+++ b/report.md\n@@ -1,2 +1 @@\n # Report\n-old line\n-second\n+new line\n@@ -10,2 +9,0 @@ ## Totals\n-x\n-y\n",
        binary: false,
        too_large: false,
        truncated: false,
      },
    };
    show();
    fireEvent.click(await screen.findByRole("button", { name: /report\.md/ }));
    const diff = await screen.findByRole("region", { name: "report.md" });
    expect(diff).not.toHaveTextContent("@@");
    expect(within(diff).getByText("Line 1")).toBeVisible();
    expect(within(diff).getByText("Removed after line 9")).toBeVisible();
    expect(within(diff).getByText("## Totals")).toBeVisible();
    expect(within(diff).getAllByText("Removed", { exact: true })).toHaveLength(
      4,
    );
    expect(within(diff).getAllByText("Added", { exact: true })).toHaveLength(1);
  });

  it("invites a first message when there are no runs", () => {
    show({ runs: [] });
    expect(
      screen.getByText(
        "No runs yet. The files a run changes appear here after it starts.",
      ),
    ).toBeVisible();
    expect(fetch).not.toHaveBeenCalled();
  });
});
