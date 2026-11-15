import { parseApi } from "../protocol/validate";

import type { ApiClient } from "./client";

/*
 * One folder of a task's workspace, read by the files panel and the empty-workspace check: its validators load with
 * the first read, off a task page's first scripts (WEB-U-472).
 */
const validators = () => import("../protocol/generated/validators.file-pages");

/** One folder of the task's workspace; `path` "" is the root. */
export async function files(
  client: ApiClient,
  id: string,
  path: string,
  after?: string,
  signal?: AbortSignal,
) {
  await validators();
  const query = new URLSearchParams({ path });
  if (after) query.set("after", after);
  return client.request("files", (value) => parseApi(value, "FilePage"), {
    parameters: { id },
    query,
    signal,
  });
}
