import { parseApi } from "../protocol/validate";

import type { ApiClient } from "./client";
import "../protocol/generated/validators.previews";

/*
 * The files panel's previews: a file's text, a changed file's diff and a spreadsheet's sheet. Only the panel asks for
 * them, and it loads on demand, so their validators live here, off a task page's first load (WEB-U-014).
 */

export function fileContent(
  client: ApiClient,
  id: string,
  path: string,
  signal?: AbortSignal,
) {
  return client.request(
    "fileContent",
    (value) => parseApi(value, "FileContent"),
    { parameters: { id }, query: new URLSearchParams({ path }), signal },
  );
}

/** One changed file of a run as a unified diff. */
export function runChangeDiff(
  client: ApiClient,
  id: string,
  run_id: string,
  path: string,
  signal?: AbortSignal,
) {
  return client.request(
    "runChangeDiff",
    (value) => parseApi(value, "FileDiff"),
    {
      parameters: { id, run_id },
      query: new URLSearchParams({ path }),
      signal,
    },
  );
}

/** One sheet of a spreadsheet as display text; the first sheet when none is named. */
export function fileSheet(
  client: ApiClient,
  id: string,
  path: string,
  sheet?: string,
  signal?: AbortSignal,
) {
  const query = new URLSearchParams({ path });
  if (sheet !== undefined) query.set("sheet", sheet);
  return client.request(
    "fileSheet",
    (value) => parseApi(value, "SheetPreview"),
    { parameters: { id }, query, signal },
  );
}
