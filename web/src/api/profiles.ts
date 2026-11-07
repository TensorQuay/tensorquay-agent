import { parseApi } from "../protocol/validate";

import type { ApiClient } from "./client";
import "../protocol/generated/validators.profiles";

/**
 * The caller's own agent profiles, then the shared ones (ADR-0012): asked for by New task and Settings, never by a
 * task's page, so their validators live here, off its first load (WEB-U-014).
 */
export function profiles(client: ApiClient, signal?: AbortSignal) {
  return client.request("profiles", (value) => parseApi(value, "Profiles"), {
    signal,
  });
}
