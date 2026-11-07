import { api, type ApiClient } from "../api/client";
import type {
  ChangeAccount,
  NewInvitation,
  NewToken,
} from "../protocol/generated/api";
import { parseApi } from "../protocol/validate";

import { settingsEndpoint } from "./operations";
import "../protocol/generated/validators.settings";

/**
 * A person's account, tokens and invitations, and the gateway's readiness: requests only the settings
 * pages make. They live here, not in the shared client, so their validators stay off the conversation page
 * (WEB-U-014).
 */
export function settingsRequests(client: ApiClient) {
  return {
    /** The gateway's readiness checks (administrators only); 503 carries the failing checks, not an error. */
    readiness: (signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("readiness"),
        (value) => parseApi(value, "ReadyDetail"),
        { query: new URLSearchParams({ detail: "1" }), accept: [503], signal },
      ),
    changeUser: (id: string, body: ChangeAccount, signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("changeUser"),
        (value) => parseApi(value, "Account"),
        { parameters: { id }, body: JSON.stringify(body), signal },
      ),
    invitations: (signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("invitations"),
        (value) => parseApi(value, "Invitations"),
        { signal },
      ),
    createInvitation: (body: NewInvitation, signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("createInvitation"),
        (value) => parseApi(value, "CreatedInvitation"),
        { body: JSON.stringify(body), signal },
      ),
    revokeInvitation: (id: string, signal?: AbortSignal) =>
      client.request(settingsEndpoint("revokeInvitation"), () => undefined, {
        parameters: { id },
        signal,
      }),
    tokens: (signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("tokens"),
        (value) => parseApi(value, "ApiTokens"),
        { signal },
      ),
    createToken: (body: NewToken, signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("createToken"),
        (value) => parseApi(value, "CreatedToken"),
        { body: JSON.stringify(body), signal },
      ),
    revokeToken: (id: string, signal?: AbortSignal) =>
      client.request(settingsEndpoint("revokeToken"), () => undefined, {
        parameters: { id },
        signal,
      }),
  };
}

export const settingsApi = settingsRequests(api);
