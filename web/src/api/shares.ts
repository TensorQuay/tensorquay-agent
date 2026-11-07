import type { ShareByEmail, ShareWorkspace } from "../protocol/generated/api";
import { parseApi } from "../protocol/validate";

import type { ApiClient } from "./client";
import "../protocol/generated/validators.shares";

/*
 * A workspace's shares, from the workspaces page's Share dialog: never a task page's, so their validators live here,
 * off its first load (WEB-U-014).
 */

export function workspaceShares(
  client: ApiClient,
  id: string,
  signal?: AbortSignal,
) {
  return client.request(
    "workspaceShares",
    (value) => parseApi(value, "WorkspaceShares"),
    { parameters: { id }, signal },
  );
}

/** Shares with the active account of exactly this email (never a search). */
export function shareWorkspaceByEmail(
  client: ApiClient,
  id: string,
  body: ShareByEmail,
  signal?: AbortSignal,
) {
  return client.request(
    "shareWorkspaceByEmail",
    (value) => parseApi(value, "WorkspaceShare"),
    { parameters: { id }, body: JSON.stringify(body), signal },
  );
}

export function shareWorkspace(
  client: ApiClient,
  id: string,
  user_id: string,
  body: ShareWorkspace,
  signal?: AbortSignal,
) {
  return client.request(
    "shareWorkspace",
    (value) => parseApi(value, "WorkspaceShare"),
    { parameters: { id, user_id }, body: JSON.stringify(body), signal },
  );
}

export function unshareWorkspace(
  client: ApiClient,
  id: string,
  user_id: string,
  signal?: AbortSignal,
) {
  return client.request("unshareWorkspace", () => undefined, {
    parameters: { id, user_id },
    signal,
  });
}
