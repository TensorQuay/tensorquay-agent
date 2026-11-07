import { act, renderHook } from "@testing-library/react";
import type { DragEvent } from "react";
import { describe, expect, it, vi } from "vitest";

import { useDrop } from "./use-drop";

const drag = (types: string[], files: File[] = []) => {
  const preventDefault = vi.fn();
  const event = { preventDefault, dataTransfer: { types, files } };
  return { event: event as unknown as DragEvent, preventDefault };
};
const file = new File(["a"], "notes.txt");

// Files dropped on the composer (WEB-U-020): only file drags count, nested elements do not end the hover early.
describe("WEB-U-020 dropping files on the composer", () => {
  it("ignores a drag that carries no files", () => {
    const onFiles = vi.fn();
    const { result } = renderHook(() => useDrop(onFiles));
    const text = drag(["text/plain"]);
    act(() => {
      result.current.handlers.onDragEnter?.(text.event);
      result.current.handlers.onDragOver?.(text.event);
      result.current.handlers.onDrop?.(text.event);
    });
    expect(result.current.dragging).toBe(false);
    expect(text.preventDefault).not.toHaveBeenCalled();
    expect(onFiles).not.toHaveBeenCalled();
  });

  it("keeps hovering over nested elements until the last leave, then drops the files", () => {
    const onFiles = vi.fn();
    const { result } = renderHook(() => useDrop(onFiles));
    const files = drag(["Files"], [file]);
    act(() => result.current.handlers.onDragEnter?.(files.event));
    act(() => result.current.handlers.onDragEnter?.(files.event));
    act(() => result.current.handlers.onDragLeave?.(files.event));
    expect(result.current.dragging).toBe(true);
    act(() => result.current.handlers.onDragLeave?.(files.event));
    expect(result.current.dragging).toBe(false);
    act(() => result.current.handlers.onDragLeave?.(files.event));
    act(() => result.current.handlers.onDragOver?.(files.event));
    expect(result.current.dragging).toBe(true);
    act(() => result.current.handlers.onDrop?.(files.event));
    expect(result.current.dragging).toBe(false);
    expect(onFiles).toHaveBeenCalledWith([file]);
    expect(files.preventDefault).toHaveBeenCalled();
  });

  it("offers no drop target where files cannot be attached", () => {
    const { result } = renderHook(() => useDrop(null));
    expect(result.current).toEqual({ dragging: false, handlers: {} });
  });
});
