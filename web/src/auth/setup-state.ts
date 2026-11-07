import { api, type ApiClient } from "../api/client";
import { parseApi } from "../protocol/validate";
import "../protocol/generated/validators.access";

/** Whether the installation still needs its first administrator: only the sign-in and setup pages ask (WEB-U-014). */
export const setupStateOf = (client: ApiClient) => (signal?: AbortSignal) =>
  client.request("setupState", (value) => parseApi(value, "Setup"), {
    signal,
  });
export const setupState = setupStateOf(api);
