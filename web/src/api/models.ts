import { parseApi } from "../protocol/validate";

import type { ApiClient } from "./client";

/*
 * The model catalogue, read once a page with a model picker is shown (use-models): its validators load with that first
 * read, off a task page's first scripts (WEB-U-472, WEB-U-014's headroom).
 */
const validators = () => import("../protocol/generated/validators.models");

/** Every model route the person may pick, with how each is answering. */
export async function models(client: ApiClient, signal?: AbortSignal) {
  await validators();
  return client.request("models", (value) => parseApi(value, "Models"), {
    signal,
  });
}
