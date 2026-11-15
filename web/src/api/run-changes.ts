import { parseApi } from "../protocol/validate";

import type { ApiClient } from "./client";

/*
 * The files one run changed, read once a run has ended: their validators load with the first read, off a task page's
 * first scripts (WEB-U-472).
 */
const validators = () => import("../protocol/generated/validators.run-changes");

/** The files one run added, modified or deleted. */
export async function runChanges(
  client: ApiClient,
  id: string,
  run_id: string,
  signal?: AbortSignal,
) {
  await validators();
  return client.request(
    "runChanges",
    (value) => parseApi(value, "RunChanges"),
    { parameters: { id, run_id }, signal },
  );
}
