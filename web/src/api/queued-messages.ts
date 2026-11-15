import { parseApi } from "../protocol/validate";

import type { ApiClient } from "./client";

/*
 * The task's held messages, read once its page is shown (use-held): their validators load with that first read, off
 * the page's first scripts (WEB-U-472, WEB-U-014's headroom).
 */
const validators = () =>
  import("../protocol/generated/validators.queued-messages");

/** The messages the gateway holds for the task, in order. */
export async function queuedMessages(
  client: ApiClient,
  id: string,
  signal?: AbortSignal,
) {
  await validators();
  return client.request(
    "queuedMessages",
    (value) => parseApi(value, "QueuedMessages"),
    { parameters: { id }, signal },
  );
}
