import type { ToolView } from "./types";

/** Reads only a requested argument from arbitrary model JSON. */
export function argument(tool: ToolView, key: string): unknown {
  return typeof tool.arguments === "object" && tool.arguments !== null
    ? Reflect.get(tool.arguments, key)
    : undefined;
}

/** Normalizes one line and preserves both ends of long model-supplied values. */
export function bounded(value: unknown, fallback = "", limit = 90): string {
  const text =
    typeof value === "string" ? value.replace(/\s+/g, " ").trim() : fallback;
  const points = Array.from(text);
  const side = Math.floor((limit - 1) / 2);
  return points.length <= limit
    ? text
    : `${points.slice(0, side).join("")}…${points.slice(-(limit - side - 1)).join("")}`;
}

/** Parses builtin write/edit count headers; unknown or failed results never imply a change. */
export function fileChange(
  tool: ToolView,
): { kind: "created" | "updated"; added: number; removed: number } | null {
  if (tool.status !== "ok" || !tool.result) return null;
  const write =
    /^(created|replaced), \d+ bytes\nadded: (\d+) lines\nremoved: (\d+) lines/.exec(
      tool.result,
    );
  const edit = /^edited [^\n]*: \d+ edits, \+(\d+) -(\d+) lines/.exec(
    tool.result,
  );
  if (tool.name === "write_file" && write)
    return {
      kind: write[1] === "created" ? "created" : "updated",
      added: Number(write[2]),
      removed: Number(write[3]),
    };
  if (tool.name === "edit_file" && edit)
    return {
      kind: "updated",
      added: Number(edit[1]),
      removed: Number(edit[2]),
    };
  return null;
}

/** The actual returned line range, falling back to explicit requested bounds while pending. */
export function readRange(
  tool: ToolView,
): { first: string; last: string } | null {
  const lines = Array.from((tool.result ?? "").matchAll(/^\s*(\d+)\t/gm));
  const first = lines[0]?.[1] ?? positiveLine(tool, "start_line");
  const last = lines.at(-1)?.[1] ?? positiveLine(tool, "end_line");
  if (first === null || last === null) return null;
  return { first: String(first), last: String(last) };
}

/** Counts rows or explicit match counts, and whether the search stopped early. */
export function searchCount(
  tool: ToolView,
): { count: number; unit: "file" | "match"; partial: boolean } | null {
  if (tool.status !== "ok" || tool.result === null) return null;
  const rows = tool.result
    .split("\n")
    .filter((line) => line && !line.startsWith("["));
  const mode = argument(tool, "output_mode") ?? "files_with_matches";
  const count =
    mode === "count"
      ? rows.reduce(
          (total, row) => total + Number(/:(\d+)$/.exec(row)?.[1] ?? 0),
          0,
        )
      : mode === "content"
        ? rows.filter((row) => /:\d+:/.test(row)).length
        : rows.length;
  return {
    count,
    unit:
      tool.name === "glob" || mode === "files_with_matches" ? "file" : "match",
    partial: /\[(partial|incomplete):/.test(tool.result),
  };
}

function positiveLine(tool: ToolView, name: string): number | null {
  const value = argument(tool, name);
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0
    ? value
    : null;
}

/** The site whose key a push needs ("example.com"), from its project link; null when it named none. */
export function keySite(tool: ToolView): string | null {
  const remote = argument(tool, "remote");
  return typeof remote === "string" && URL.canParse(remote)
    ? new URL(remote).hostname.replace(/^www\./, "")
    : null;
}

/** A push that failed for lack of a stored credential, or because the repository refused it. */
export function needsGitCredential(tool: ToolView): boolean {
  return (
    tool.name === "git_push" &&
    tool.status === "error" &&
    /^git_push_(credential_required|authentication):/.test(tool.result ?? "")
  );
}
