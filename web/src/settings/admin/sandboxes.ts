import { api, type ApiClient } from "../../api/client";
import type {
  AdminSandbox,
  AdminSandboxLeftBehind,
  AdminSandboxes,
} from "../../protocol/generated/api";
import { parseAdminApi } from "../../protocol/validate-admin";
import { settingsEndpoint } from "../operations";
import "../../protocol/generated/validators.settings";

export type SandboxListing = AdminSandboxes;
export type SandboxRow = AdminSandbox;
export type LeftBehind = AdminSandboxLeftBehind;

/** The Sandboxes page's gateway operations (the Sandboxes page design note, §2); tests replace it with a fake. */
export type SandboxesApi = {
  /** Every running sandbox, a page of 200; `after` is the previous page's `next_after`. */
  sandboxes(after?: string, signal?: AbortSignal): Promise<SandboxListing>;
  /** Asks the gateway to stop an idle sandbox; `key` is the action's Idempotency-Key. */
  stop(conversationId: string, key: string): Promise<void>;
};

export function gatewaySandboxesFor(client: ApiClient): SandboxesApi {
  return {
    sandboxes: (after, signal) =>
      client.request(
        settingsEndpoint("adminSandboxes"),
        (value) => parseAdminApi(value, "AdminSandboxes"),
        {
          ...(after ? { query: new URLSearchParams({ after }) } : {}),
          signal,
        },
      ),
    stop: async (conversationId, key) => {
      await client.request(
        settingsEndpoint("adminSandboxStop"),
        (value) => parseAdminApi(value, "AdminSandboxStop"),
        { parameters: { conversation_id: conversationId }, key },
      );
    },
  };
}

export const gatewaySandboxes = gatewaySandboxesFor(api);
