/** Where the sandbox mounts a task's workspace; the files panel and the changes list name paths below it. */
export const WORKSPACE_ROOT = "/workspace";

/**
 * A path an agent gave a tool, as the workspace path the page opens (WEB-U-083): the sandbox's own absolute form
 * (/workspace/…) and relative forms (./x, x) read the same, empty and "." segments go, and ".." steps back. Anything
 * outside the workspace (another folder of the sandbox, or a step above it) is null: the page never opens it.
 */
export function workspacePath(raw: string): string | null {
  let path = raw.trim();
  if (path.startsWith("/")) {
    if (path !== WORKSPACE_ROOT && !path.startsWith(`${WORKSPACE_ROOT}/`))
      return null;
    path = path.slice(WORKSPACE_ROOT.length);
  }
  const parts: string[] = [];
  for (const part of path.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (parts.length === 0) return null;
      parts.pop();
      continue;
    }
    parts.push(part);
  }
  return parts.length ? parts.join("/") : null;
}
