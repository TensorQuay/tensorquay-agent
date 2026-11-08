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

import type { FileEntry, User } from "../protocol/generated/api";
import { contextLoaded } from "../test/context";
import { conversationId } from "../test/frames";
import { settleLazy } from "../test/lazy";
import { LIMITS } from "../test/user";

import { FilesPanel } from "./FilesPanel";

const modified_at = "2026-01-01T00:00:00Z";
const entry = (
  name: string,
  kind: FileEntry["kind"] = "file",
  size = 12,
): FileEntry => ({ name, kind, size, modified_at });
type Answer = { status?: number; body: unknown };
let folders: Record<string, Answer[]> = {};
let contents: Record<string, Answer> = {};
const fetch = vi.fn<typeof globalThis.fetch>();
const refusal = (status: number, code: string): Answer => ({
  status,
  body: { error: { code, message: "Refused.", retryable: false } },
});
const page = (
  path: string,
  items: FileEntry[],
  next_after: string | null = null,
): Answer => ({ body: { path, items, next_after } });
const text = (path: string, value: string | null, extra = {}): Answer => ({
  body: {
    path,
    size: value?.length ?? 4096,
    modified_at,
    etag: '"1-2-3"',
    binary: value === null,
    truncated: false,
    text: value,
    ...extra,
  },
});

beforeEach(() => {
  folders = {
    "": [
      page("", [
        entry("src", "dir", 0),
        entry("README.md"),
        entry("latest", "link"),
      ]),
    ],
    src: [page("src", [entry("app.py")])],
  };
  contents = {};
  fetch.mockReset().mockImplementation((input) => {
    const url = new URL(input as string, "http://localhost");
    const path = url.searchParams.get("path") ?? "";
    const answer = url.pathname.endsWith("/files/content")
      ? contents[path]
      : folders[path]?.shift();
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

function wrapper(role?: User["role"]) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  // The page loaded the session before the panel; the panel only reads it.
  if (role)
    cache.setQueryData<User>(["session"], {
      id: "user-1",
      email: "person@example.com",
      role,
      internet: "none",
      limits: LIMITS,
      features: { agent_teams: false },
    });
  return function Providers({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={cache}>{children}</QueryClientProvider>;
  };
}
function show(
  props: Partial<Parameters<typeof FilesPanel>[0]> = {},
  role?: User["role"],
) {
  const onClose = vi.fn();
  const view = render(
    <FilesPanel
      conversationId={conversationId}
      running={false}
      version="idle"
      runs={[]}
      context={{ kind: "none" }}
      onClose={onClose}
      {...props}
    />,
    { wrapper: wrapper(role) },
  );
  return { ...view, onClose };
}
const listed = async () =>
  within(await screen.findByRole("list", { name: /^Files in/ }))
    .getAllByRole("listitem")
    .map((item) => item.textContent);

describe("WEB-U-022 files panel", () => {
  // The body scrolls, and in the panel floating below 1280 px it often must: the keyboard reaches it (WCAG 2.1.1;
  // axe scrollable-region-focusable), as the tabs pattern gives a tab panel.
  it("lets the keyboard reach and scroll the panel's body", () => {
    show();
    expect(screen.getByRole("tabpanel")).toHaveAttribute("tabindex", "0");
  });

  // .git and other dotfiles were listed first, which is noise. They wait behind one quiet button.
  it("hides dotfiles until asked, and says how many", async () => {
    folders[""] = [
      page("", [entry(".git", "dir", 0), entry(".env"), entry("README.md")]),
    ];
    show();
    expect(await listed()).toEqual(["README.md12 B"]);
    fireEvent.click(
      screen.getByRole("button", { name: "Show hidden files (2)" }),
    );
    expect(await listed()).toEqual([".git", ".env12 B", "README.md12 B"]);
    fireEvent.click(screen.getByRole("button", { name: "Hide hidden files" }));
    expect(await listed()).toEqual(["README.md12 B"]);
  });

  it("lists a folder, opens a subfolder and returns through the path", async () => {
    show();
    expect(await listed()).toEqual([
      "src",
      "README.md12 B",
      "latestLink, not followed",
    ]);
    expect(screen.queryByRole("button", { name: /latest/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "src" }));
    await waitFor(async () => expect(await listed()).toEqual(["app.py12 B"]));
    const path = screen.getByRole("navigation", { name: "Folder path" });
    expect(within(path).getByText("src")).toHaveAttribute(
      "aria-current",
      "page",
    );
    folders[""] = [page("", [entry("README.md")])];
    fireEvent.click(within(path).getByRole("button", { name: "Workspace" }));
    await waitFor(async () =>
      expect(await listed()).toEqual(["README.md12 B"]),
    );
  });

  it("previews text with a download link and returns to the folder", async () => {
    contents["README.md"] = text("README.md", "# Title\nSecond line\n");
    show();
    fireEvent.click(await screen.findByRole("button", { name: /^README\.md/ }));
    const preview = await screen.findByRole("region", { name: "README.md" });
    // A Markdown file renders as a document; the renderer loads first (WEB-U-014).
    await settleLazy();
    expect(
      within(preview).getByRole("heading", { name: "Title" }),
    ).toBeVisible();
    expect(within(preview).getByText("Second line")).toBeVisible();
    expect(
      within(preview).getByRole("link", { name: "Download" }),
    ).toHaveAttribute(
      "href",
      `/api/v1/conversations/${conversationId}/files/download?path=README.md`,
    );
    fireEvent.click(
      within(screen.getByRole("navigation", { name: "Folder path" })).getByRole(
        "button",
        { name: "Workspace" },
      ),
    );
    expect(
      await screen.findByRole("list", { name: /^Files in/ }),
    ).toBeVisible();
  });

  it.each([
    [
      text("README.md", null),
      "This file can't be previewed. Download it to open it.",
    ],
    [text("README.md", "", { size: 0 }), "This file is empty."],
    [
      text("README.md", "partial", { size: 900_000, truncated: true }),
      "Showing the first 512 kB.",
    ],
    [refusal(404, "not_found"), "This file is no longer there."],
  ])("says plainly what a preview cannot show", async (answer, copy) => {
    contents["README.md"] = answer;
    show();
    fireEvent.click(await screen.findByRole("button", { name: /^README\.md/ }));
    expect(await screen.findByText(copy)).toBeVisible();
  });

  it("loads more entries from the cursor", async () => {
    folders[""] = [
      page("", [entry("a.txt")], "a.txt"),
      page("", [entry("b.txt")]),
    ];
    show();
    fireEvent.click(
      await screen.findByRole("button", { name: "Show more files" }),
    );
    await waitFor(async () =>
      expect(await listed()).toEqual(["a.txt12 B", "b.txt12 B"]),
    );
    expect(fetch.mock.calls[1]?.[0] as string).toContain("after=a.txt");
    expect(
      screen.queryByRole("button", { name: "Show more files" }),
    ).toBeNull();
  });

  it.each([
    [
      page("", []),
      "No files yet. Files you attach and files the agent writes appear here.",
    ],
    [
      refusal(409, "files_unavailable"),
      "Files are not available in this environment.",
    ],
    [refusal(500, "internal"), "The files could not be loaded."],
  ])("shows the empty and refused states", async (answer, copy) => {
    folders[""] = [answer];
    show();
    expect(await screen.findByText(copy)).toBeVisible();
  });

  it("offers the way back when a folder disappears", async () => {
    folders.src = [refusal(404, "not_found")];
    show();
    fireEvent.click(await screen.findByRole("button", { name: "src" }));
    expect(
      await screen.findByText("This folder is no longer there."),
    ).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", { name: "Back to the workspace" }),
    );
    expect(
      await screen.findByRole("list", { name: /^Files in/ }),
    ).toBeVisible();
  });

  it("warns while the agent works and refreshes when the run changes", async () => {
    const { rerender } = show({ running: true, version: "run-1:running" });
    expect(
      screen.getByText("The agent is working; files may change."),
    ).toBeVisible();
    await listed();
    folders[""] = [page("", [entry("result.csv")])];
    rerender(
      <FilesPanel
        conversationId={conversationId}
        running={false}
        version="run-1:completed"
        runs={[]}
        context={{ kind: "none" }}
        onClose={vi.fn()}
      />,
    );
    await waitFor(async () =>
      expect(await listed()).toEqual(["result.csv12 B"]),
    );
    expect(
      screen.queryByText("The agent is working; files may change."),
    ).toBeNull();
  });

  it("closes from its button and with Escape", async () => {
    const { onClose } = show();
    const panel = screen.getByRole("complementary", { name: "Files" });
    fireEvent.click(within(panel).getByRole("button", { name: "Close panel" }));
    fireEvent.keyDown(await screen.findByRole("button", { name: "src" }), {
      key: "Escape",
    });
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});

describe("WEB-U-022 files panel focus", () => {
  it("keeps focus inside the panel when an entry opens", async () => {
    contents["README.md"] = text("README.md", "# Title\n");
    const { onClose } = show();
    fireEvent.click(await screen.findByRole("button", { name: /^README\.md/ }));
    await screen.findByRole("region", { name: "README.md" });
    const panel = screen.getByRole("complementary", { name: "Files" });
    expect(panel).toContainElement(document.activeElement as HTMLElement);
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });

  // WEB-U-087: Escape in a preview opened wide closes only that dialog, not the panel under it.
  it("leaves Escape inside a preview opened wide to that dialog", async () => {
    contents["report.html"] = text("report.html", "<p>hi</p>");
    const { onClose } = show({ target: { path: "report.html", request: 1 } });
    const panel = screen.getByRole("complementary", { name: "Files" });
    fireEvent.click(
      await within(panel).findByRole("button", { name: "Expand" }),
    );
    const dialog = await screen.findByRole("dialog", { name: "report.html" });
    fireEvent.keyDown(
      within(dialog).getByRole("button", { name: "Close dialog" }),
      { key: "Escape" },
    );
    expect(onClose).not.toHaveBeenCalled();
  });
});

describe("WEB-U-024 files panel tabs", () => {
  it("switches between the workspace files and the runs' changes", async () => {
    show({ runs: [] });
    const tabs = screen.getByRole("tablist", { name: "Task views" });
    expect(within(tabs).getByRole("tab", { name: "Files" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await listed();
    fireEvent.click(within(tabs).getByRole("tab", { name: "Changes" }));
    expect(within(tabs).getByRole("tab", { name: "Changes" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    // The changes view loads when its tab first opens (WEB-U-014).
    await settleLazy();
    await waitFor(() =>
      expect(
        screen.getByRole("tabpanel", { name: "Changes" }),
      ).toHaveTextContent(
        "No runs yet. The files a run changes appear here after it starts.",
      ),
    );
    expect(
      screen.queryByRole("navigation", { name: "Folder path" }),
    ).toBeNull();
  });
});

describe("WEB-U-046 the Context tab", () => {
  it("shows what the latest run loaded, with arrow keys across all three tabs and nothing to refresh", async () => {
    show({
      context: {
        kind: "report",
        startedAt: "2026-01-01T00:00:00Z",
        report: {
          instructions: [{ path: "AGENTS.md", truncated: false }],
          skills: [],
          skipped: [],
        },
      },
    });
    const tabs = screen.getByRole("tablist", { name: "Task views" });
    const tab = (name: string) => within(tabs).getByRole("tab", { name });
    fireEvent.keyDown(tab("Files"), { key: "ArrowLeft" });
    expect(tab("Context")).toHaveAttribute("aria-selected", "true");
    expect(tab("Context")).toHaveFocus();
    const panel = screen.getByRole("tabpanel", { name: "Context" });
    expect(await within(panel).findByText("AGENTS.md")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Refresh files" })).toBeNull();
    fireEvent.keyDown(tab("Context"), { key: "ArrowRight" });
    expect(tab("Files")).toHaveFocus();
    fireEvent.keyDown(tab("Files"), { key: "ArrowRight" });
    expect(tab("Changes")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("button", { name: "Refresh files" })).toBeVisible();
  });

  it.each([
    [{ kind: "none" } as const],
    [{ kind: "loading" } as const],
    [{ kind: "nothing", startedAt: "2026-01-01T00:00:00Z" } as const],
  ])("leaves the tab out for %o", (context) => {
    show({ context });
    const tabs = screen.getByRole("tablist", { name: "Task views" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(["Files", "Changes"]);
  });

  it("shows the Files tab when the Context tab it was on goes away", () => {
    show({ context: { kind: "none" }, tab: "context", onTab: vi.fn() });
    expect(screen.getByRole("tab", { name: "Files" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });
  // Why a folder was left out is for an administrator, who can fix it; anyone else sees it wasn't loaded.
  it.each([
    ["admin", true],
    ["member", false],
  ] as const)("as %s, shows why a folder was left out: %s", (role, reason) => {
    show(
      {
        context: {
          kind: "report",
          startedAt: "2026-01-01T00:00:00Z",
          report: {
            instructions: [],
            skills: [],
            skipped: contextLoaded.skipped,
          },
        },
        tab: "context",
        onTab: vi.fn(),
      },
      role,
    );
    const panel = screen.getByRole("tabpanel", { name: "Context" });
    expect(panel).toHaveTextContent("pdf-report");
    if (reason) expect(panel).toHaveTextContent("No SKILL.md in the folder.");
    else expect(panel).not.toHaveTextContent(/SKILL\.md/);
  });
});

// A finished turn's Open shows that file's preview, and asking again shows it again.
describe("WEB-U-022 opening a file from elsewhere", () => {
  it("opens the target's folder and preview, again on a new request", async () => {
    contents["src/app.py"] = text("src/app.py", "print('hello')\n");
    const view = show({ target: { path: "src/app.py", request: 1 } });
    expect(await screen.findByRole("region", { name: "app.py" })).toBeVisible();
    fireEvent.click(
      within(screen.getByRole("navigation", { name: "Folder path" })).getByRole(
        "button",
        { name: "Workspace" },
      ),
    );
    await listed();
    view.rerender(
      <FilesPanel
        conversationId={conversationId}
        running={false}
        version="idle"
        runs={[]}
        context={{ kind: "none" }}
        onClose={view.onClose}
        target={{ path: "src/app.py", request: 2 }}
      />,
    );
    expect(await screen.findByRole("region", { name: "app.py" })).toBeVisible();
  });
});

// WEB-U-083: a path outside the workspace says so plainly, and nothing is fetched for it.
describe("WEB-U-083 a file outside the workspace", () => {
  it("says it can't be shown here and asks for nothing", async () => {
    show({ target: { path: "/etc/motd", request: 1, outside: true } });
    expect(
      await screen.findByText(
        "That file is outside this task’s workspace, so it can’t be shown here.",
      ),
    ).toBeVisible();
    expect(screen.queryByText("This file is no longer there.")).toBeNull();
    expect(JSON.stringify(fetch.mock.calls)).not.toContain("motd");
  });
});

// The Context tab is there only when a run loaded something to show.
