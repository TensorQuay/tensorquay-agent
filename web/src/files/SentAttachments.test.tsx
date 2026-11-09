// WEB-U-464: a sent message shows its images as pictures and opens any attached file's preview.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { conversationId } from "../test/frames";

import { SentAttachments } from "./SentAttachments";

describe("WEB-U-464 sent attachments", () => {
  // As the file preview's tests do: the API's own fetch answers the download with a PNG's first bytes, or fails.
  const fetch = vi.fn();
  beforeEach(() => {
    fetch.mockReset();
    vi.stubGlobal("fetch", fetch);
    Object.assign(URL, {
      createObjectURL: vi.fn(() => "blob:sent/1"),
      revokeObjectURL: vi.fn(),
    });
  });
  afterEach(() => vi.unstubAllGlobals());

  it("shows an image as a picture from its bytes that opens its preview, named for screen readers", async () => {
    fetch.mockImplementation(() =>
      Promise.resolve(
        new Response(new Uint8Array([137, 80, 78, 71]), { status: 200 }),
      ),
    );
    const open = vi.fn();
    render(
      <SentAttachments
        paths={["uploads/chart.png", "uploads/notes.pdf"]}
        conversationId={conversationId}
        onOpenFile={open}
      />,
    );
    // The placeholder gives way to the picture once its bytes are read.
    await waitFor(() =>
      expect(screen.getByRole("img", { name: "chart.png" })).toHaveAttribute(
        "src",
        "blob:sent/1",
      ),
    );
    expect(fetch).toHaveBeenCalledTimes(1);
    const asked = new URL(String(fetch.mock.calls[0]?.[0]), "http://localhost");
    expect(asked.pathname).toBe(
      `/api/v1/conversations/${conversationId}/files/download`,
    );
    expect(asked.searchParams.get("path")).toBe("uploads/chart.png");
    fireEvent.click(screen.getByRole("button", { name: "Open chart.png" }));
    expect(open).toHaveBeenCalledWith("uploads/chart.png");
    // Any other file stays a chip that opens its preview too.
    fireEvent.click(screen.getByRole("button", { name: "Open notes.pdf" }));
    expect(open).toHaveBeenLastCalledWith("uploads/notes.pdf");
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
      <SentAttachments
        paths={["uploads/gone.webp"]}
        conversationId={conversationId}
        onOpenFile={vi.fn()}
      />,
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
    render(<SentAttachments paths={["uploads/a.png", "uploads/b.txt"]} />);
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByText("a.png")).toBeVisible();
    expect(screen.getByText("b.txt")).toBeVisible();
  });
});
