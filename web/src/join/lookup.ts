import { api, type ApiClient } from "../api/client";
import { parseApi } from "../protocol/validate";
import "../protocol/generated/validators.access";

/** Who an invitation is for: only the join page asks (WEB-U-014). */
export const lookupInvitationOf =
  (client: ApiClient) => (token: string, signal?: AbortSignal) =>
    client.request("lookupInvitation", (value) => parseApi(value, "Invitee"), {
      body: JSON.stringify({ token }),
      signal,
    });
export const lookupInvitation = lookupInvitationOf(api);
