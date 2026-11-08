import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Upload } from "../protocol/generated/api";
import { conversationId } from "../test/frames";
import { Composer } from "../transcript/Composer";

import {
  MAX_ATTACHMENTS,
  MAX_FILE_BYTES,
  useAttachments,
} from "./use-attachments";

/** An upload that stores each file under its own name, in the order the composer sends them. */
function uploads() {
  const files: File[] = [];
  const upload = vi.fn((_id: string, file: File) => {
    files.push(file);
    return Promise.resolve<Upload>({
      path: `uploads/${file.name}`,
      name: file.name,
      size: file.size,
    });
  });
  return { upload, files };
}

function Harness({ upload }: { upload: ReturnType<typeof uploads>["upload"] }) {
  const attachments = useAttachments(conversationId, upload);
  return (
    <Composer
      run={{ id: null, status: "idle" }}
      sandbox="ready"
      sending={false}
      cancelling={false}
      onStop={() => Promise.resolve()}
      onSend={() => Promise.resolve(true)}
      attachments={attachments}
    />
  );
}

const file = (name: string, type: string, size = 8) =>
  new File([new Uint8Array(size)], name, { type });
/** A clipboard as the browser hands it to a paste: file items for files, text for text; true when the default stood. */
const paste = (files: File[], text = "") =>
  fireEvent.paste(screen.getByRole("textbox", { name: "Message" }), {
    clipboardData: {
      items: files.map((item) => ({
        kind: "file",
        type: item.type,
        getAsFile: () => item,
      })),
      files,
      types: [...(text ? ["text/plain"] : []), ...files.map(() => "Files")],
      getData: () => text,
    },
  });
const chips = () => screen.getByRole("list", { name: "Attached files" });
const objectUrls = () => {
  let serial = 0;
  const create = vi.fn(() => `blob:preview-${++serial}`);
  const revoke = vi.fn();
  vi.stubGlobal(
    "URL",
    Object.assign(URL, { createObjectURL: create, revokeObjectURL: revoke }),
  );
  return { create, revoke };
};
afterEach(() => vi.unstubAllGlobals());

// WEB-U-427: what the clipboard holds goes the chooser's way: an image or a file becomes an attachment at once, text
// stays text, and the same limits refuse the same way.
describe("WEB-U-427 pasting into the composer", () => {
  it("attaches a pasted image as pasted-image-N.png with a thumbnail, and frees the thumbnail on remove", async () => {
    const urls = objectUrls();
    const { upload, files } = uploads();
    render(<Harness upload={upload} />);
    expect(paste([file("image.png", "image/png")])).toBe(false);
    expect(paste([file("image.png", "image/png")])).toBe(false);
    const list = chips();
    expect(within(list).getByText("pasted-image-1.png")).toBeVisible();
    expect(within(list).getByText("pasted-image-2.png")).toBeVisible();
    // Uploads go one at a time, in paste order.
    await waitFor(() =>
      expect(files.map((item) => item.name)).toEqual([
        "pasted-image-1.png",
        "pasted-image-2.png",
      ]),
    );
    expect(files[0]?.type).toBe("image/png");
    // The thumbnail is the image itself, decorative beside its name.
    const thumbnails = within(list).getAllByRole("presentation");
    expect(thumbnails.map((image) => image.getAttribute("src"))).toEqual([
      "blob:preview-1",
      "blob:preview-2",
    ]);
    expect(urls.create).toHaveBeenCalledWith(files[0]);
    fireEvent.click(
      within(list).getByRole("button", { name: "Remove pasted-image-1.png" }),
    );
    expect(urls.revoke).toHaveBeenCalledWith("blob:preview-1");
    expect(within(list).queryByText("pasted-image-1.png")).toBeNull();
  });

  it("names a pasted JPEG by its type and keeps a pasted file's own name", async () => {
    objectUrls();
    const { upload, files } = uploads();
    render(<Harness upload={upload} />);
    paste([file("image.jpeg", "image/jpeg")]);
    paste([file("notes.txt", "text/plain"), file("chart.png", "image/png")]);
    const list = chips();
    expect(within(list).getByText("pasted-image-1.jpg")).toBeVisible();
    expect(within(list).getByText("notes.txt")).toBeVisible();
    expect(within(list).getByText("chart.png")).toBeVisible();
    await waitFor(() =>
      expect(files.map((item) => item.name)).toEqual([
        "pasted-image-1.jpg",
        "notes.txt",
        "chart.png",
      ]),
    );
    // A file that isn't an image has no thumbnail; the named image has one.
    expect(within(list).getAllByRole("presentation")).toHaveLength(2);
  });

  it("leaves pasted text to the textbox", () => {
    const { upload } = uploads();
    render(<Harness upload={upload} />);
    expect(paste([], "Read the report")).toBe(true);
    expect(upload).not.toHaveBeenCalled();
    expect(screen.queryByRole("list", { name: "Attached files" })).toBeNull();
  });

  it("refuses a pasted file over the limit as the chooser does, without uploading", () => {
    objectUrls();
    const { upload } = uploads();
    render(<Harness upload={upload} />);
    paste([file("image.png", "image/png", MAX_FILE_BYTES + 1)]);
    const chip = within(chips()).getByText("pasted-image-1.png").closest("li");
    expect(chip).toHaveAttribute("data-status", "failed");
    expect(
      within(chip as HTMLElement).getByText("Too large: up to 25 MB per file"),
    ).toBeVisible();
    expect(upload).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  });

  it("refuses pasted files past the count limit with the chooser's words", () => {
    objectUrls();
    const { upload } = uploads();
    render(<Harness upload={upload} />);
    fireEvent.change(screen.getByLabelText("Choose files to attach"), {
      target: {
        files: Array.from({ length: MAX_ATTACHMENTS }, (_, index) =>
          file(`part-${index}.txt`, "text/plain"),
        ),
      },
    });
    paste([file("image.png", "image/png"), file("notes.txt", "text/plain")]);
    expect(screen.getByRole("status")).toHaveTextContent(
      "2 files were not added: up to 32 per message",
    );
    expect(within(chips()).queryByText("pasted-image-1.png")).toBeNull();
  });

  it("attaches nothing while files cannot be attached, and gives an image chosen with + the same thumbnail", () => {
    objectUrls();
    const { upload } = uploads();
    const { rerender } = render(<Harness upload={upload} />);
    fireEvent.change(screen.getByLabelText("Choose files to attach"), {
      target: { files: [file("photo.png", "image/png")] },
    });
    expect(within(chips()).getByRole("presentation")).toHaveAttribute(
      "src",
      "blob:preview-1",
    );
    rerender(
      <Composer
        run={{ id: null, status: "idle" }}
        sandbox="creating"
        sending={false}
        cancelling={false}
        onStop={() => Promise.resolve()}
        onSend={() => Promise.resolve(true)}
      />,
    );
    expect(paste([file("image.png", "image/png")])).toBe(true);
    expect(screen.queryByText("pasted-image-1.png")).toBeNull();
  });
});
