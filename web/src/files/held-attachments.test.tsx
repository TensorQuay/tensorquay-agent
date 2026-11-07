import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Upload } from "../protocol/generated/api";
import { conversationId } from "../test/frames";

import { useAttachments } from "./use-attachments";

// §3 step 1b: New task has no task yet to upload into. Its files are held, shown as chips, and uploaded to the task
// once it exists, before the first message names them.
describe("WEB-U-020 files held until the task exists", () => {
  it("holds files without uploading, then uploads them in order to the new task and returns their paths", async () => {
    const upload = vi.fn((_id: string, file: File) =>
      Promise.resolve<Upload>({
        name: file.name,
        path: `uploads/${file.name}`,
        size: file.size,
      }),
    );
    const { result } = renderHook(() => useAttachments(null, upload));
    const a = new File(["a"], "a.csv");
    const b = new File(["bb"], "b.txt");
    act(() => result.current.add([a, b]));
    expect(result.current.items.map((item) => item.status)).toEqual([
      "held",
      "held",
    ]);
    expect(upload).not.toHaveBeenCalled();
    expect(result.current.uploading).toBe(false);
    let paths: string[] | null = null;
    await act(async () => {
      paths = await result.current.uploadHeld(conversationId);
    });
    expect(upload.mock.calls.map(([id, file]) => [id, file.name])).toEqual([
      [conversationId, "a.csv"],
      [conversationId, "b.txt"],
    ]);
    expect(paths).toEqual(["uploads/a.csv", "uploads/b.txt"]);
    expect(result.current.items.map((item) => item.status)).toEqual([
      "done",
      "done",
    ]);
  });

  it("answers null when a held file fails, keeping the others, so the send can be retried", async () => {
    const upload = vi
      .fn((_id: string, file: File) =>
        Promise.resolve<Upload>({
          name: file.name,
          path: `uploads/${file.name}`,
          size: file.size,
        }),
      )
      .mockRejectedValueOnce(new Error("lost"));
    const { result } = renderHook(() => useAttachments(null, upload));
    act(() => result.current.add([new File(["a"], "a.csv")]));
    let paths: string[] | null = [];
    await act(async () => {
      paths = await result.current.uploadHeld(conversationId);
    });
    expect(paths).toBeNull();
    expect(result.current.items[0]?.status).toBe("failed");
  });
});
