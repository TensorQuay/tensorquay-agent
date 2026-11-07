/** Where a unified diff's hunk is, in the changed file's own line numbers (WEB-U-164). */
export type Hunk =
  /** It leaves lines `from` to `to` (inclusive) in the new file. */
  | { kind: "lines"; from: number; to: number; section: string | null }
  /** It only removes lines, after line `after` of the new file (0: at its start). */
  | { kind: "removed"; after: number; section: null | string };

/**
 * `@@ -start[,count] +start[,count] @@ [section]`. A line number has at most nine digits (a file of a billion lines
 * is beyond any diff the gateway sends; anything longer is refused, never read wrong).
 */
const HEADER = /^@@ -\d{1,9}(?:,\d{1,9})? \+(\d{1,9})(?:,(\d{1,9}))? @@(.*)$/;

/** A hunk header read into where its change is; null for anything that isn't one. */
export function parseHunk(header: string): Hunk | null {
  const found = HEADER.exec(header);
  if (!found) return null;
  const start = Number(found[1]);
  // A count left out means one line (the unified diff format).
  const count = found[2] === undefined ? 1 : Number(found[2]);
  const section = found[3]?.trim() || null;
  return count === 0
    ? { kind: "removed", after: start, section }
    : { kind: "lines", from: start, to: start + count - 1, section };
}
