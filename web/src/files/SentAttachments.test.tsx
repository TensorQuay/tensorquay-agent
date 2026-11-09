// WEB-U-464: a sent message shows its images as pictures and opens any attached file's preview.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { conversationId } from "../test/frames";

import { SentAttachments } from "./SentAttachments";

describe("WEB-U-464 sent attachments", () => {
  it("shows an image as a picture that opens its preview, named for screen readers", () => {
    const open = vi.fn();
    render(
      <SentAttachments
        paths={["uploads/chart.png", "uploads/notes.pdf"]}
        conversationId={conversationId}
        onOpenFile={open}
      />,
    );
    const picture = screen.getByRole("img", { name: "chart.png" });
    expect(picture.getAttribute("src")).toMatch(
      new RegExp(
        `/api/v1/conversations/${conversationId}/files/view\\?path=uploads%2Fchart\\.png`,
      ),
    );
    expect(picture).toHaveAttribute("loading", "lazy");
    fireEvent.click(screen.getByRole("button", { name: "Open chart.png" }));
    expect(open).toHaveBeenCalledWith("uploads/chart.png");
    // Any other file stays a chip that opens its preview too.
    fireEvent.click(screen.getByRole("button", { name: "Open notes.pdf" }));
    expect(open).toHaveBeenLastCalledWith("uploads/notes.pdf");
  });

  it("falls back to the file's chip when its picture can't load", () => {
    render(
      <SentAttachments
        paths={["uploads/gone.webp"]}
        conversationId={conversationId}
        onOpenFile={vi.fn()}
      />,
    );
    fireEvent.error(screen.getByRole("img", { name: "gone.webp" }));
    expect(screen.queryByRole("img", { name: "gone.webp" })).toBeNull();
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
