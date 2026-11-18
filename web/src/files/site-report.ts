/**
 * What a previewed page reports it couldn't load (18.1, built web apps preview as they are). The gateway puts a small
 * reporter first in each page served under a site pass; it posts at most one message per kind to the panel:
 * `{type: "tqa-site-report", kind, paths, more}`. The page could forge one, so a report is read as text about the page
 * itself and nothing more: known kinds only, at most ten paths of 120 characters, rendered as text.
 */
export type SiteReport = {
  /** Loads the page's policy refused at the server's root (a build with root paths in its scripts). */
  root?: { paths: string[]; more: number };
  /** Files under the page's folder that failed to load. */
  missing?: { paths: string[]; more: number };
  /** The app's `#root` or `#app` stayed empty, with nothing failing (often a browser-history router). */
  empty?: true;
};

const PATHS = 10;
const LENGTH = 120;

/** One message from the frame, as a part of a report; `null` for anything else. */
export function siteReportPart(data: unknown): SiteReport | null {
  if (typeof data !== "object" || data === null) return null;
  const message = data as Record<string, unknown>;
  if (message.type !== "tqa-site-report") return null;
  if (message.kind === "empty") return { empty: true };
  if (message.kind !== "root" && message.kind !== "missing") return null;
  const paths = Array.isArray(message.paths)
    ? message.paths
        .filter((path): path is string => typeof path === "string")
        .map((path) => path.slice(0, LENGTH))
        .filter((path) => path.length > 0)
        .slice(0, PATHS)
    : [];
  const more =
    typeof message.more === "number" &&
    Number.isInteger(message.more) &&
    message.more > 0
      ? Math.min(message.more, 1_000)
      : 0;
  // A missing-files report names what's missing, or says nothing.
  if (message.kind === "missing" && paths.length === 0) return null;
  return { [message.kind]: { paths, more } };
}

/** The one line the panel shows, by priority: root paths, then missing files, then a page that drew nothing. */
export function siteReportLine(
  report: SiteReport,
): "root" | "missing" | "empty" | null {
  if (report.root) return "root";
  if (report.missing) return "missing";
  if (report.empty) return "empty";
  return null;
}
