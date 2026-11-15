import { parseApi } from "../protocol/validate";

import type { ApiClient } from "./client";

/*
 * A task's stored key uses (ADR-0037), which the page reads only when their line is opened: their validators load with
 * the first read, off a task page's first scripts (WEB-U-472, WEB-U-014's headroom).
 */
const validators = () => import("../protocol/generated/validators.key-uses");

/** One page of the task's stored key uses, newest first; `before` is the previous page's `next`. */
export async function keyUses(
  client: ApiClient,
  id: string,
  before: string | null,
  signal?: AbortSignal,
) {
  await validators();
  const query = new URLSearchParams({ limit: "100" });
  if (before) query.set("before", before);
  return client.request("keyUses", (value) => parseApi(value, "TaskKeyUses"), {
    parameters: { id },
    query,
    signal,
  });
}
