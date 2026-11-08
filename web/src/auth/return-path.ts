/** One path segment of an app page: letters, digits, `-`, `_` and `.`, never starting with a dot. */
const SEGMENT = "[A-Za-z0-9_-][A-Za-z0-9._-]*";
/**
 * The app's own pages a person may come back to after signing in again (UI-GUIDELINES §14; a pass over the pages
 * before 17.0; WEB-U-257): a task, Channels and a channel, Workspaces, and Settings with its sections and their pages.
 */
const APP_PAGE = new RegExp(
  `^/(?:c/[A-Za-z0-9-]+|channels(?:/[A-Za-z0-9-]+)?|workspaces|settings(?:/${SEGMENT}){0,3})$`,
);

/**
 * Only the app's own pages are accepted after sign-in; anything else (another site, a protocol-relative or script
 * URL, the API, sign-in itself) returns to New task. A query is left behind: only the page comes back. The path is
 * decoded first, so an encoded segment (`%2e%2e`, which a browser reads as `..`) is judged as the browser will read it,
 * and a path that doesn't decode is refused.
 */
export function returnPath(path: string | null): string {
  if (path === null) return "/";
  let decoded: string;
  try {
    decoded = decodeURIComponent(path);
  } catch {
    return "/";
  }
  const page = decoded.split("?", 1)[0] ?? "";
  return page === "/" || APP_PAGE.test(page) ? page : "/";
}
