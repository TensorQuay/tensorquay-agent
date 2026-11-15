import { parseApi } from "../protocol/validate";

import type { ApiClient } from "./client";

/*
 * The accounts (Settings and a private task's sharing), read only where they are shown: their validators load with the
 * first read, off a task page's first scripts (WEB-U-472).
 */
const validators = () => import("../protocol/generated/validators.accounts");

/** Every account of the install. */
export async function users(client: ApiClient, signal?: AbortSignal) {
  await validators();
  return client.request("users", (value) => parseApi(value, "Accounts"), {
    signal,
  });
}
