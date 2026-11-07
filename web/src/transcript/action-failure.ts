import { ApiError } from "../api/error";
import { english, type MessageKey, type Translate } from "../i18n/translate";

import { type Failure, knownFailure } from "./failure-keys";

/**
 * A failure's sentence: what happened, then what to do (WEB-U-236). The same words the transcript shows for a failed
 * run, so one failure never reads two ways on one page.
 */
export function failureSentence(
  failure: Failure,
  t: Translate = english,
): string {
  return t("failure.sentence", {
    reason: t(failure.reason),
    next: t(failure.next),
  });
}

/** Uses the same recovery wording in the prompt and the conversation bar. */
export function notSentMessage(
  reason: string | undefined,
  t: Translate = english,
): string {
  return t("failure.notSent", { reason: reason ?? t("failure.retry") });
}

/**
 * The send route's own refusals a person can meet (openapi's POST …/messages), and the request's own failures. Each is
 * what happened and what to do, as a run's failures are.
 */
const REFUSALS: Readonly<Record<string, Failure>> = {
  conversation_deleted: {
    reason: "refuse.taskGone",
    next: "refuse.taskGoneNext",
  },
  not_found: { reason: "refuse.taskGone", next: "refuse.taskGoneNext" },
  workspace_not_ready: {
    reason: "refuse.workspaceNotReady",
    next: "refuse.waitThenSendNext",
  },
  workspace_busy: {
    reason: "refuse.workspaceBusy",
    next: "refuse.workspaceBusyNext",
  },
  body_too_large: { reason: "refuse.tooLong", next: "refuse.tooLongNext" },
  unauthorized: { reason: "refuse.signedOut", next: "refuse.signedOutNext" },
  session_required: {
    reason: "refuse.signedOut",
    next: "refuse.signedOutNext",
  },
  forbidden: { reason: "refuse.forbidden", next: "refuse.forbiddenNext" },
  idempotency_key_reused: {
    reason: "refuse.alreadySent",
    next: "refuse.alreadySentNext",
  },
  invalid_request: {
    reason: "refuse.unreadable",
    next: "refuse.reloadSendNext",
  },
  attachment_missing: {
    reason: "refuse.attachment",
    next: "refuse.attachmentNext",
  },
  template_unavailable: {
    reason: "refuse.environment",
    next: "refuse.environmentNext",
  },
  profile_unusable: { reason: "refuse.profile", next: "refuse.profileNext" },
  network: { reason: "refuse.network", next: "refuse.checkArrivedNext" },
  timeout: { reason: "refuse.timeout", next: "refuse.checkArrivedNext" },
};

/** A code this page doesn't know: still what to do, never a bare "something went wrong". */
const UNKNOWN: Failure = {
  reason: "refuse.unknown",
  next: "refuse.unknownNext",
};

/**
 * Server diagnostics stay off the page; the stable code selects the words. A send refusal has its own; any protocol
 * error code has the run's (failure-keys.ts, typed and tested against the protocol); anything else is UNKNOWN.
 */
export function actionFailure(code: string): Failure {
  return REFUSALS[code] ?? knownFailure(code, "") ?? UNKNOWN;
}

/**
 * A storage quota notice's sentence (N4): the workspace over quota while the run goes on (retryable), or past the
 * margin so the run stopped; the task's saved state over quota (an administrator must act). Null for any other code.
 */
export function quotaKey(code: string, retryable: boolean): MessageKey | null {
  if (code === "workspace_quota")
    return retryable ? "quota.workspaceOver" : "quota.workspaceStopped";
  return code === "state_quota" ? "quota.stateStopped" : null;
}

/** The reason a request failed, in the reader's language; local errors keep their own (already web-owned) copy. */
export function requestFailureMessage(
  error: Error,
  t: Translate = english,
): string {
  if (!(error instanceof ApiError)) return error.message;
  return failureSentence(actionFailure(error.code), t);
}
