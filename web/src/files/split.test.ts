import { afterEach, describe, expect, it, vi } from "vitest";

import {
  CHAT_LEAST,
  DEFAULT_SHARE,
  FILE_LEAST,
  SPLIT_WIDTH,
  bounds,
  clampShare,
  keyedShare,
  rememberShare,
  rememberedShare,
} from "./split";
import { DOCK_WIDTH } from "./use-task-layout";

// WEB-U-494 (18.1): the split view's numbers. The file takes 60 % of the task layout by default; the chat keeps at
// least 360 px and the file at least 400; the split exists from where the panel docks (900 px). The divider's keys
// move it 2 % (10 % with Shift), Home and End go to the limits, Enter returns to 60 %. The share is remembered per
// user in this browser, and a storage that throws is no error.
describe("WEB-U-494 the split's numbers", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
  });

  it("splits where the panel docks, the chat at least 360 and the file at least 400", () => {
    expect(SPLIT_WIDTH).toBe(DOCK_WIDTH);
    expect(DEFAULT_SHARE).toBe(0.6);
    expect([CHAT_LEAST, FILE_LEAST]).toEqual([360, 400]);
    // At 900 the default leaves the chat exactly 360.
    expect(Math.round(900 * (1 - DEFAULT_SHARE))).toBe(CHAT_LEAST);
    expect(bounds(1020)).toEqual({ least: 400 / 1020, most: 1 - 360 / 1020 });
    expect(clampShare(0.9, 1020)).toBeCloseTo(1 - 360 / 1020);
    expect(clampShare(0.2, 1020)).toBeCloseTo(400 / 1020);
    expect(clampShare(0.6, 1020)).toBe(0.6);
    // A remembered wide share opens sensibly on a narrower layout.
    expect(clampShare(0.8, 900)).toBeCloseTo(0.6);
  });

  it("moves by the divider's keys, within the limits", () => {
    expect(keyedShare("ArrowLeft", false, 0.6, 1020)).toBeCloseTo(0.62);
    expect(keyedShare("ArrowRight", false, 0.6, 1020)).toBeCloseTo(0.58);
    expect(keyedShare("ArrowLeft", true, 0.6, 1020)).toBeCloseTo(
      1 - 360 / 1020,
    );
    expect(keyedShare("ArrowRight", true, 0.6, 1660)).toBeCloseTo(0.5);
    expect(keyedShare("Home", false, 0.6, 1020)).toBeCloseTo(1 - 360 / 1020);
    expect(keyedShare("End", false, 0.6, 1020)).toBeCloseTo(400 / 1020);
    expect(keyedShare("Enter", false, 0.45, 1020)).toBe(0.6);
    expect(keyedShare("a", false, 0.45, 1020)).toBeNull();
  });

  it("remembers the share per user, and a throwing storage is no error", () => {
    expect(rememberedShare("user-1")).toBe(DEFAULT_SHARE);
    rememberShare("user-1", 0.7);
    expect(rememberedShare("user-1")).toBe(0.7);
    expect(rememberedShare("user-2")).toBe(DEFAULT_SHARE);
    expect(localStorage.getItem("tqa.files-split.user-1")).toBe("0.7");
    localStorage.setItem("tqa.files-split.user-3", "nonsense");
    expect(rememberedShare("user-3")).toBe(DEFAULT_SHARE);
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    expect(rememberedShare("user-1")).toBe(DEFAULT_SHARE);
    expect(() => rememberShare("user-1", 0.5)).not.toThrow();
  });
});
