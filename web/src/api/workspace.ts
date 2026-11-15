import type {
  ForkWorkspace,
  NewWorkspace,
  UpdateWorkspace,
} from "../protocol/generated/api";
import { parseApi } from "../protocol/validate";

import type { ApiClient } from "./client";

/*
 * One workspace (the workspaces page, a team's lead page): read and changed only there, so its validators load with
 * the first such request, off a task page's first scripts (WEB-U-472, WEB-U-014's headroom).
 */
const validators = () => import("../protocol/generated/validators.workspace");

export async function workspace(
  client: ApiClient,
  id: string,
  signal?: AbortSignal,
) {
  await validators();
  return client.request("workspace", (value) => parseApi(value, "Workspace"), {
    parameters: { id },
    signal,
  });
}

export async function createWorkspace(
  client: ApiClient,
  body: NewWorkspace,
  signal?: AbortSignal,
) {
  await validators();
  return client.request(
    "createWorkspace",
    (value) => parseApi(value, "Workspace"),
    { body: JSON.stringify(body), signal },
  );
}

export async function updateWorkspace(
  client: ApiClient,
  id: string,
  body: UpdateWorkspace,
  signal?: AbortSignal,
) {
  await validators();
  return client.request(
    "updateWorkspace",
    (value) => parseApi(value, "Workspace"),
    { parameters: { id }, body: JSON.stringify(body), signal },
  );
}

/** Starts a copy in the background; the new workspace is `forking` until it is ready. */
export async function forkWorkspace(
  client: ApiClient,
  id: string,
  body: ForkWorkspace,
  signal?: AbortSignal,
) {
  await validators();
  return client.request(
    "forkWorkspace",
    (value) => parseApi(value, "Workspace"),
    { parameters: { id }, body: JSON.stringify(body), signal },
  );
}
