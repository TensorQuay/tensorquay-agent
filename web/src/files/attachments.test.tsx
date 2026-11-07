import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/client";
import type { UploadOptions } from "../api/upload";
import type { Upload } from "../protocol/generated/api";
import { conversationId } from "../test/frames";
import { Composer } from "../transcript/Composer";

import {
  MAX_ATTACHMENTS,
  MAX_FILE_BYTES,
  useAttachments,
} from "./use-attachments";

type Call = {
  file: File;
  options: UploadOptions;
  resolve: (value: Upload) => void;
  reject: (error: Error) => void;
};

/** A controllable upload: each call waits until the test answers it. */
function uploads() {
  const calls: Call[] = [];
  const upload = vi.fn(
    (_id: string, file: File, options: UploadOptions = {}) =>
      new Promise<Upload>((resolve, reject) => {
        calls.push({ file, options, resolve, reject });
      }),
  );
  return { upload, calls };
}

function Harness({
  upload,
  onSend,
  limit,
}: {
  upload: ReturnType<typeof uploads>["upload"];
  onSend: (text: string, paths: string[]) => Promise<boolean>;
  /** The installation's per-file limit, as sign-in and me carry it (GW-I-142). */
  limit?: number;
}) {
  const attachments = useAttachments(conversationId, upload, limit);
  return (
    <Composer
      run={{ id: null, status: "idle" }}
      sandbox="ready"
      sending={false}
      cancelling={false}
      onStop={() => Promise.resolve()}
      attachments={attachments}
      onSend={async (text) => {
        const sent = await onSend(text, attachments.paths);
        if (sent) attachments.clear();
        return sent;
      }}
    />
  );
}

const file = (name: string, size = 8) => {
  const value = new File(["a,b\n1,2\n"], name);
  Object.defineProperty(value, "size", { value: size });
  return value;
};
const choose = (...files: File[]) =>
  fireEvent.change(screen.getByLabelText("Choose files to attach"), {
    target: { files },
  });

describe("WEB-U-020 attachments in the composer", () => {
  it("uploads a chosen file with progress, then sends its path", async () => {
    const { upload, calls } = uploads();
    const onSend = vi.fn(() => Promise.resolve(true));
    render(<Harness upload={upload} onSend={onSend} />);
    expect(
      screen.getByRole("button", { name: "Attach files" }),
    ).toHaveAccessibleDescription("Up to 25 MB per file, 32 files per message");
    choose(file("Q3 report.csv", 2048));
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Summarise the report" },
    });
    const chips = screen.getByRole("list", { name: "Attached files" });
    expect(within(chips).getByText("Q3 report.csv")).toBeVisible();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(screen.getByText("Waiting for uploads to finish")).toBeVisible();

    act(() => calls[0]?.options.onProgress?.(1024, 2048));
    expect(
      within(chips).getByRole("progressbar", { name: "Q3 report.csv" }),
    ).toHaveAttribute("aria-valuenow", "50");

    act(() =>
      calls[0]?.resolve({
        path: "uploads/Q3_report.csv",
        name: "Q3_report.csv",
        size: 2048,
      }),
    );
    await waitFor(() =>
      expect(within(chips).getByText("Q3_report.csv")).toBeVisible(),
    );
    expect(within(chips).getByText("2 kB")).toBeVisible();
    expect(within(chips).queryByRole("progressbar")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() =>
      expect(onSend).toHaveBeenCalledWith("Summarise the report", [
        "uploads/Q3_report.csv",
      ]),
    );
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: "Attached files" })).toBeNull(),
    );
  });

  it("refuses an oversized file before uploading and blocks Send until it is removed", () => {
    const { upload } = uploads();
    render(<Harness upload={upload} onSend={() => Promise.resolve(true)} />);
    choose(file("video.mov", MAX_FILE_BYTES + 1));
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Look" },
    });
    expect(upload).not.toHaveBeenCalled();
    expect(screen.getByText("Too large: up to 25 MB per file")).toBeVisible();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(
      screen.getByText("Remove the files that could not be uploaded"),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Remove video.mov" }));
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
  });

  // GW-I-142: the installation's own limit, from the signed-in account, not a fixed 25 MB.
  it("refuses a file over the installation's limit and names that limit", async () => {
    const { upload, calls } = uploads();
    const limit = 2 * 1024 * 1024;
    render(
      <Harness
        upload={upload}
        onSend={() => Promise.resolve(true)}
        limit={limit}
      />,
    );
    expect(
      screen.getByRole("button", { name: "Attach files" }),
    ).toHaveAccessibleDescription(/2 MB/);
    choose(file("big.csv", limit + 1));
    expect(upload).not.toHaveBeenCalled();
    expect(screen.getByText("Too large: up to 2 MB per file")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Remove big.csv" }));
    choose(file("small.csv", limit));
    await waitFor(() => expect(calls).toHaveLength(1));
  });

  it.each([
    ["uploads_full", "The workspace upload space is full"],
    ["upload_too_large", "Too large: up to 25 MB per file"],
    ["files_unavailable", "Files are not available in this environment"],
    ["network", "Upload failed"],
  ])("shows the refusal %s in plain words", async (code, copy) => {
    const { upload, calls } = uploads();
    render(<Harness upload={upload} onSend={() => Promise.resolve(true)} />);
    choose(file("notes.txt"));
    act(() => calls[0]?.reject(new ApiError(409, code, "Refused.")));
    expect(await screen.findByText(copy)).toBeVisible();
  });

  it("takes at most 32 files per message and says how many were left out", () => {
    const { upload } = uploads();
    render(<Harness upload={upload} onSend={() => Promise.resolve(true)} />);
    choose(
      ...Array.from({ length: MAX_ATTACHMENTS + 1 }, (_, index) =>
        file(`part-${index}.txt`),
      ),
    );
    const chips = screen.getByRole("list", { name: "Attached files" });
    expect(within(chips).getAllByRole("listitem")).toHaveLength(
      MAX_ATTACHMENTS,
    );
    expect(
      screen.getByText("1 file was not added: up to 32 per message"),
    ).toBeVisible();
    choose(file("late.txt"));
    expect(within(chips).getAllByRole("listitem")).toHaveLength(
      MAX_ATTACHMENTS,
    );
    expect(within(chips).queryByText("late.txt")).toBeNull();
  });

  // WEB-U-022 flake: the gateway numbers duplicate names as uploads arrive, so uploads go one at a time.
  it("uploads one file at a time in attach order, so duplicates are numbered in that order", async () => {
    const { upload, calls } = uploads();
    const onSend = vi.fn(() => Promise.resolve(true));
    render(<Harness upload={upload} onSend={onSend} />);
    choose(file("Q3 sales.csv"), file("Q3 sales.csv"), file("notes.txt"));
    expect(upload).toHaveBeenCalledOnce();
    act(() =>
      calls[0]?.resolve({
        path: "uploads/Q3_sales.csv",
        name: "Q3_sales.csv",
        size: 8,
      }),
    );
    await waitFor(() => expect(upload).toHaveBeenCalledTimes(2));
    act(() => calls[1]?.reject(new ApiError(409, "uploads_full", "Full.")));
    await waitFor(() => expect(upload).toHaveBeenCalledTimes(3));
    expect(calls.map((call) => call.file.name)).toEqual([
      "Q3 sales.csv",
      "Q3 sales.csv",
      "notes.txt",
    ]);
    act(() =>
      calls[2]?.resolve({
        path: "uploads/notes.txt",
        name: "notes.txt",
        size: 8,
      }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Remove Q3 sales.csv" }),
    );
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Summarise" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() =>
      expect(onSend).toHaveBeenCalledWith("Summarise", [
        "uploads/Q3_sales.csv",
        "uploads/notes.txt",
      ]),
    );
  });

  it("starts the next upload when the running one is removed, and never starts a removed waiting one", async () => {
    const { upload, calls } = uploads();
    render(<Harness upload={upload} onSend={() => Promise.resolve(true)} />);
    choose(file("a.bin"), file("b.bin"), file("c.bin"));
    fireEvent.click(screen.getByRole("button", { name: "Remove c.bin" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove a.bin" }));
    expect(calls[0]?.options.signal?.aborted).toBe(true);
    await waitFor(() => expect(upload).toHaveBeenCalledTimes(2));
    expect(calls[1]?.file.name).toBe("b.bin");
    act(() =>
      calls[1]?.resolve({ path: "uploads/b.bin", name: "b.bin", size: 8 }),
    );
    await waitFor(() => expect(screen.getByText("b.bin")).toBeVisible());
    choose(file("d.bin"));
    await waitFor(() => expect(upload).toHaveBeenCalledTimes(3));
    expect(calls.map((call) => call.file.name)).toEqual([
      "a.bin",
      "b.bin",
      "d.bin",
    ]);
  });

  it("cancels an upload that is removed", () => {
    const { upload, calls } = uploads();
    render(<Harness upload={upload} onSend={() => Promise.resolve(true)} />);
    choose(file("big.bin"));
    fireEvent.click(screen.getByRole("button", { name: "Remove big.bin" }));
    expect(calls[0]?.options.signal?.aborted).toBe(true);
    expect(screen.queryByRole("list", { name: "Attached files" })).toBeNull();
  });

  it("accepts files dropped on the composer", () => {
    const { upload } = uploads();
    render(<Harness upload={upload} onSend={() => Promise.resolve(true)} />);
    const form = screen.getByRole("form", { name: "Message composer" });
    const dropped = file("drop.csv");
    const dataTransfer = { types: ["Files"], files: [dropped] };
    fireEvent.dragEnter(form, { dataTransfer });
    fireEvent.dragOver(form, { dataTransfer });
    expect(screen.getByText("Drop files to attach")).toBeVisible();
    fireEvent.drop(form, { dataTransfer });
    expect(upload).toHaveBeenCalledOnce();
    expect(screen.queryByText("Drop files to attach")).toBeNull();
    expect(screen.getByText("drop.csv")).toBeVisible();
  });

  it("offers no attach control where there is no task yet", () => {
    render(
      <Composer
        run={{ id: null, status: "idle" }}
        sandbox="ready"
        sending={false}
        cancelling={false}
        onSend={() => Promise.resolve(true)}
        onStop={() => Promise.resolve()}
      />,
    );
    expect(screen.queryByRole("button", { name: "Attach files" })).toBeNull();
  });
});
