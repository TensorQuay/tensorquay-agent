import type { MessageKey } from "../i18n/translate";
import type { GitPushFailure } from "../protocol/generated/protocol";

import type { ToolView } from "./types";

/**
 * Every way a push can fail, in plain words under its step (WEB-U-094): what happened and what to do, with no git words
 * and no code. Typed over GitPushFailure, so a new failure without words fails the typecheck. The two credential
 * failures ("key") keep their link to the person's Keys (WEB-U-038). key_use_unrecorded is GW-I-149's.
 */
export const PUSH_FAILURES: Readonly<
  Record<GitPushFailure | "key_use_unrecorded", MessageKey | "key">
> = {
  git_push_credential_required: "key",
  git_push_authentication: "key",
  git_push_snapshot_changed: "activity.pushSnapshotChanged",
  git_push_rejected: "activity.pushRejected",
  git_push_outcome_unknown: "activity.pushOutcomeUnknown",
  git_push_expired: "activity.pushExpired",
  invalid_args: "activity.pushInvalid",
  proxy_authentication: "activity.pushProxy",
  denied: "activity.pushDenied",
  egress_denied: "activity.pushEgress",
  output_too_large: "activity.pushTooLarge",
  timeout: "activity.pushTimeout",
  key_use_unrecorded: "activity.pushKeyUnrecorded",
};

/** A failed push's plain words, when its code has them and they aren't the credential link. */
export function pushFailure(tool: ToolView): MessageKey | null {
  if (tool.name !== "git_push" || tool.status !== "error") return null;
  const code = /^([a-z0-9_]+):/.exec(tool.result ?? "")?.[1] ?? "";
  const words = (PUSH_FAILURES as Readonly<Record<string, string>>)[code];
  return words && words !== "key" ? (words as MessageKey) : null;
}
