import { parseApi } from "../protocol/validate";

import type { ApiClient } from "./client";

/*
 * A desktop task's stored screen notices (ADR-0039 section 6), read only on a desktop task: their validators load with
 * the first read, off a task page's first scripts (WEB-U-472).
 */
const validators = () =>
  import("../protocol/generated/validators.screen-notices");

/** A desktop task's stored screen notices, oldest first. */
export async function screenNotices(
  client: ApiClient,
  id: string,
  signal?: AbortSignal,
) {
  await validators();
  return client.request(
    "screenNotices",
    (value) => parseApi(value, "ScreenNotices"),
    { parameters: { id }, signal },
  );
}
