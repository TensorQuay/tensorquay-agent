import { DOCK_WIDTH } from "./use-task-layout";

/**
 * The Files panel's split view (18.1, WEB-U-494): Expand gives the file a share of the task layout beside the chat,
 * from the width where the panel docks. The chat keeps at least CHAT_LEAST and the file at least FILE_LEAST; the share
 * is remembered per user in this browser.
 */
export const SPLIT_WIDTH = DOCK_WIDTH;
export const DEFAULT_SHARE = 0.6;
export const CHAT_LEAST = 360;
export const FILE_LEAST = 400;
/** The divider's arrow keys move it by this share, or BIG_STEP with Shift. */
const STEP = 0.02;
const BIG_STEP = 0.1;

/** The file's least and most share of a layout `width` px wide. */
export function bounds(width: number): { least: number; most: number } {
  return { least: FILE_LEAST / width, most: 1 - CHAT_LEAST / width };
}

/** `share` within the layout's bounds (a share remembered on a wide screen opens sensibly on a narrower one). */
export function clampShare(share: number, width: number): number {
  if (!(width > 0)) return share;
  const { least, most } = bounds(width);
  return Math.min(Math.max(share, least), most);
}

/** The share the divider's `key` gives (null for a key it doesn't take). */
export function keyedShare(
  key: string,
  shift: boolean,
  share: number,
  width: number,
): number | null {
  const step = shift ? BIG_STEP : STEP;
  const { least, most } = bounds(width);
  switch (key) {
    // The divider moves left: the file, on its right, grows.
    case "ArrowLeft":
      return clampShare(share + step, width);
    case "ArrowRight":
      return clampShare(share - step, width);
    case "Home":
      return most;
    case "End":
      return least;
    case "Enter":
      return DEFAULT_SHARE;
    default:
      return null;
  }
}

const key = (user: string) => `tqa.files-split.${user}`;

/** The share this user last chose in this browser, else the default; a storage that throws is no error. */
export function rememberedShare(user: string): number {
  try {
    const stored = Number(localStorage.getItem(key(user)));
    return stored > 0 && stored < 1 ? stored : DEFAULT_SHARE;
  } catch {
    return DEFAULT_SHARE;
  }
}

/** Remembers the share for this user in this browser, where storage allows. */
export function rememberShare(user: string, share: number) {
  try {
    localStorage.setItem(key(user), String(Math.round(share * 100) / 100));
  } catch {
    // Private windows and blocked storage: the split still works, at the default next time.
  }
}
