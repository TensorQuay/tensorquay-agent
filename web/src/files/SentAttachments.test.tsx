// WEB-U-464: a sent message shows its images as pictures and opens any attached file's preview.
import { readFileSync } from "node:fs";

import {
  focusManager,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { api } from "../api/client";
import { conversationId } from "../test/frames";

import { SentAttachments } from "./SentAttachments";

/** The gateway's answer to a file's metadata read (GET …/files/content): a binary file of `size` bytes. */
const metadata = (path: string, size: number) =>
  Response.json({
    path,
    size,
    modified_at: "2026-11-09T07:00:00Z",
    etag: `"${path}-${size}"`,
    binary: true,
    truncated: false,
    text: null,
  });
const png = () =>
  new Response(new Uint8Array([137, 80, 78, 71]), { status: 200 });
/** Lets every read the page would start now start, and answer. */
const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 50));
  });

describe("WEB-U-464 sent attachments", () => {
  // As the file preview's tests do: the API's own fetch answers each file's metadata (4 bytes unless `sizes` says
  // otherwise) and its download with a PNG's first bytes, or fails.
  const fetch = vi.fn();
  const created = vi.fn(() => "blob:sent/1");
  const revoked = vi.fn();
  let sizes: Record<string, number> = {};
  let client: QueryClient;
  const wrap = (node: ReactNode) => (
    <QueryClientProvider client={client}>{node}</QueryClientProvider>
  );
  /** Waits for the picture itself: its placeholder is an image of the same name until the bytes are read. */
  const drawn = (name: string) =>
    waitFor(() =>
      expect(screen.getByRole("img", { name })).toHaveAttribute(
        "src",
        "blob:sent/1",
      ),
    );
  const asked = (ending: string) =>
    fetch.mock.calls
      .map(([input]) => new URL(String(input), "http://localhost"))
      .filter((url) => url.pathname.endsWith(ending))
      .map((url) => url.searchParams.get("path"));
  beforeEach(() => {
    sizes = {};
    client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    fetch.mockReset();
    fetch.mockImplementation((input: unknown) => {
      const url = new URL(String(input), "http://localhost");
      const path = url.searchParams.get("path") ?? "";
      return Promise.resolve(
        url.pathname.endsWith("/files/content")
          ? metadata(path, sizes[path] ?? 4)
          : png(),
      );
    });
    vi.stubGlobal("fetch", fetch);
    created.mockClear();
    revoked.mockClear();
    Object.assign(URL, { createObjectURL: created, revokeObjectURL: revoked });
  });
  afterEach(() => {
    focusManager.setFocused(undefined);
    client.clear();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("shows an image as a picture from its bytes that opens its preview, named for screen readers", async () => {
    const open = vi.fn();
    render(
      wrap(
        <SentAttachments
          paths={["uploads/chart.png", "uploads/notes.pdf"]}
          conversationId={conversationId}
          onOpenFile={open}
        />,
      ),
    );
    // The placeholder gives way to the picture once its bytes are read.
    await waitFor(() =>
      expect(screen.getByRole("img", { name: "chart.png" })).toHaveAttribute(
        "src",
        "blob:sent/1",
      ),
    );
    expect(asked("/files/download")).toEqual(["uploads/chart.png"]);
    fireEvent.click(screen.getByRole("button", { name: "Open chart.png" }));
    expect(open).toHaveBeenCalledWith("uploads/chart.png");
    // Any other file stays a chip that opens its preview too.
    fireEvent.click(screen.getByRole("button", { name: "Open notes.pdf" }));
    expect(open).toHaveBeenLastCalledWith("uploads/notes.pdf");
  });

  it("lists a message's files once, pictures first, under one name", async () => {
    render(
      wrap(
        <SentAttachments
          paths={["uploads/notes.pdf", "uploads/chart.png"]}
          conversationId={conversationId}
          onOpenFile={vi.fn()}
        />,
      ),
    );
    const lists = screen.getAllByRole("list", { name: "Attached files" });
    expect(lists).toHaveLength(1);
    const items = within(lists[0] as HTMLElement).getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(
      within(items[0] as HTMLElement).getByRole("button", {
        name: "Open chart.png",
      }),
    ).toBeVisible();
    expect(items[1]).toHaveTextContent("notes.pdf");
    await drawn("chart.png");
  });

  it("doesn't fetch a picture over the Files panel's 10 MiB limit: its chip opens the preview instead", async () => {
    sizes["uploads/scan.png"] = 10 * 1024 * 1024 + 1;
    render(
      wrap(
        <SentAttachments
          paths={["uploads/scan.png"]}
          conversationId={conversationId}
          onOpenFile={vi.fn()}
        />,
      ),
    );
    await waitFor(() =>
      expect(screen.queryByRole("img", { name: "scan.png" })).toBeNull(),
    );
    expect(
      screen.getByRole("button", { name: "Open scan.png" }),
    ).toHaveTextContent("scan.png");
    expect(asked("/files/download")).toEqual([]);
  });

  it("reads a picture's bytes once per version, in the Files panel preview's own cache", async () => {
    const shown = () =>
      wrap(
        <SentAttachments
          paths={["uploads/chart.png"]}
          conversationId={conversationId}
          onOpenFile={vi.fn()}
        />,
      );
    const first = render(shown());
    await drawn("chart.png");
    first.unmount();
    // The task's page shown again (another task and back): the picture comes from the cache.
    render(shown());
    await drawn("chart.png");
    // Nor does the window's focus coming back (the app's queries read again on focus by default).
    act(() => {
      focusManager.setFocused(false);
      focusManager.setFocused(true);
    });
    await settle();
    expect(asked("/files/content")).toEqual(["uploads/chart.png"]);
    expect(asked("/files/download")).toEqual(["uploads/chart.png"]);
    // The preview reads the same entries, so opening the picture's file reads nothing again.
    expect(
      client.getQueryData([
        "file-image",
        conversationId,
        "uploads/chart.png",
        '"uploads/chart.png-4"',
      ]),
    ).toBeInstanceOf(ArrayBuffer);
    expect(
      client.getQueryData(["file", conversationId, "uploads/chart.png"]),
    ).toMatchObject({ size: 4 });
  });

  it("leaves no object URL behind when the picture goes before its bytes are read", async () => {
    // The bytes' read waits until released, so the picture goes while it is under way.
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const reading = vi.fn(async () => {
      await gate;
      return new Uint8Array([137, 80, 78, 71]).buffer;
    });
    vi.spyOn(api, "fileBlob").mockResolvedValue({
      arrayBuffer: reading,
    } as unknown as Blob);
    const view = render(
      wrap(
        <SentAttachments
          paths={["uploads/chart.png"]}
          conversationId={conversationId}
          onOpenFile={vi.fn()}
        />,
      ),
    );
    await waitFor(() => expect(reading).toHaveBeenCalled());
    view.unmount();
    release();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(created.mock.calls.length).toBe(revoked.mock.calls.length);
  });

  it("revokes the picture's object URL when it goes", async () => {
    const view = render(
      wrap(
        <SentAttachments
          paths={["uploads/chart.png"]}
          conversationId={conversationId}
          onOpenFile={vi.fn()}
        />,
      ),
    );
    await waitFor(() =>
      expect(screen.getByRole("img", { name: "chart.png" })).toHaveAttribute(
        "src",
        "blob:sent/1",
      ),
    );
    view.unmount();
    expect(created).toHaveBeenCalledTimes(1);
    expect(revoked).toHaveBeenCalledWith("blob:sent/1");
  });

  it("keeps a drawn picture when a later read of its file fails (a refresh while the sandbox is away)", async () => {
    render(
      wrap(
        <SentAttachments
          paths={["uploads/chart.png"]}
          conversationId={conversationId}
          onOpenFile={vi.fn()}
        />,
      ),
    );
    await drawn("chart.png");
    fetch.mockImplementation(() =>
      Promise.resolve(
        Response.json(
          { error: { code: "sandbox_unavailable", message: "away" } },
          { status: 503 },
        ),
      ),
    );
    // As the Files panel's refresh does.
    await act(async () => {
      await client.invalidateQueries({ queryKey: ["file", conversationId] });
    });
    await settle();
    expect(screen.getByRole("img", { name: "chart.png" })).toBeVisible();
  });

  it("reads nothing for a picture until it nears the screen", async () => {
    const watching: ((entries: { isIntersecting: boolean }[]) => void)[] = [];
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(
          callback: (entries: { isIntersecting: boolean }[]) => void,
        ) {
          watching.push(callback);
        }
        observe = vi.fn();
        disconnect = vi.fn();
      },
    );
    render(
      wrap(
        <SentAttachments
          paths={["uploads/chart.png"]}
          conversationId={conversationId}
          onOpenFile={vi.fn()}
        />,
      ),
    );
    await settle();
    expect(fetch).not.toHaveBeenCalled();
    expect(watching).toHaveLength(1);
    act(() => watching[0]?.([{ isIntersecting: true }]));
    await drawn("chart.png");
    expect(asked("/files/download")).toEqual(["uploads/chart.png"]);
  });

  it("falls back to the file's chip when its picture can't be read", async () => {
    fetch.mockImplementation(() =>
      Promise.resolve(
        Response.json(
          { error: { code: "not_found", message: "gone" } },
          { status: 404 },
        ),
      ),
    );
    render(
      wrap(
        <SentAttachments
          paths={["uploads/gone.webp"]}
          conversationId={conversationId}
          onOpenFile={vi.fn()}
        />,
      ),
    );
    // The picture's placeholder is a button too: wait until the read fails and the chip takes its place.
    await waitFor(() =>
      expect(screen.queryByRole("img", { name: "gone.webp" })).toBeNull(),
    );
    expect(
      screen.getByRole("button", { name: "Open gone.webp" }),
    ).toHaveTextContent("gone.webp");
  });

  it("keeps plain chips when the page can't open files (no panel)", () => {
    render(
      wrap(<SentAttachments paths={["uploads/a.png", "uploads/b.txt"]} />),
    );
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByText("a.png")).toBeVisible();
    expect(screen.getByText("b.txt")).toBeVisible();
  });

  it("draws nothing as a control where files can't be opened: no pointer, no hover tint", async () => {
    const { container } = render(
      wrap(
        <SentAttachments
          paths={["uploads/chart.png", "uploads/b.txt"]}
          conversationId={conversationId}
        />,
      ),
    );
    await drawn("chart.png");
    expect(screen.queryByRole("button")).toBeNull();
    // The chip's control class (a pointer and the hover tint) is on buttons only.
    expect(container.querySelector(".attachment-open")).toBeNull();
    // A picture's frame shows the zoom-in pointer only as a button.
    const css = readFileSync("src/app/files.css", "utf8").replace(
      /\/\*[\s\S]*?\*\//g,
      "",
    );
    const frame = css.match(/(?:^|\})\s*\.sent-picture\s*\{([^}]*)\}/)?.[1];
    expect(frame).toBeDefined();
    expect(frame).not.toMatch(/cursor/);
    expect(css).toMatch(/button\.sent-picture\s*\{[^}]*cursor:\s*zoom-in/);
  });
});
