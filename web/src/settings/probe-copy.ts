import { ApiError } from "../api/error";
import type { MessageValues } from "../i18n/message";
import type { MessageKey } from "../i18n/translate";

import type { ProbeResult } from "./admin/routes";

type Observed = ProbeResult["observed"];
type FailureCode = NonNullable<Observed["failure"]>["code"];

/** A check step's outcome in words (`tool_call`, `round_trip`). */
export const STEPS: Readonly<Record<Observed["tool_call"], MessageKey>> = {
  passed: "models.stepPassed",
  failed: "models.stepFailed",
  not_declared: "models.stepNotDeclared",
  not_run: "models.stepNotRun",
};

/** Each failure the gateway can observe; a code added to the contract fails the typecheck until it has a sentence. */
export const FAILURES: Readonly<Record<FailureCode, MessageKey>> = {
  auth_rejected: "models.failAuthRejected",
  bad_tool_call: "models.failBadToolCall",
  cancelled: "models.failCancelled",
  configuration: "models.failConfiguration",
  no_tool_call: "models.failNoToolCall",
  not_found: "models.failNotFound",
  endpoint_missing: "models.failEndpointMissing",
  not_streaming: "models.failNotStreaming",
  protocol_error: "models.failProtocolError",
  rate_limited: "models.failRateLimited",
  round_trip_failed: "models.failRoundTripFailed",
  strict_line_breaks_lost: "models.failStrictLineBreaksLost",
  timeout: "models.failTimeout",
  unreachable: "models.failUnreachable",
  upstream_error: "models.failUpstreamError",
  context_too_small: "models.failContextTooSmall",
  usage_refused: "models.failUsageRefused",
};

export const WARNINGS: Readonly<
  Record<Observed["warnings"][number], MessageKey>
> = {
  argument_mismatch: "models.warnArgumentMismatch",
  line_breaks_unverified: "models.warnLineBreaksUnverified",
  stream_mislabelled: "models.warnStreamMislabelled",
  swallowed_calls: "models.warnSwallowedCalls",
  thinks_unasked: "models.warnThinksUnasked",
  usage_missing: "models.warnUsageMissing",
  // Shown as the verdict itself, with the speed, by RouteCheck (a first-hour check before 17.0), never from this table.
  slow_answers: "models.checkVerifiedSlowly",
  // Shown with the sizes and its one click by RouteCheck (WEB-U-269), never from this table.
  context_window_differs: "models.warnContextWindowDiffers",
};

/** A failure from a newer gateway still reads as plain words (UI-GUIDELINES §14). */
export const failureKey = (code: string): MessageKey =>
  Object.hasOwn(FAILURES, code)
    ? FAILURES[code as FailureCode]
    : "models.failUnknown";

/** Why a check could not run, with the gateway's wait when it gave one (Retry-After). */
export function probeRefusal(error: unknown): [MessageKey, MessageValues] {
  if (!(error instanceof ApiError)) return ["models.checkError", {}];
  if (error.code === "key_missing") return ["models.checkKeyMissing", {}];
  if (error.code === "probe_running") return ["models.checkRunning", {}];
  if (error.status === 429)
    return error.retryAfter
      ? ["models.checkTooSoon", { seconds: error.retryAfter }]
      : ["models.checkTooSoonNoTime", {}];
  if (error.status === 504) return ["models.checkTimeout", {}];
  if (error.status === 404) return ["models.checkGone", {}];
  return ["models.checkError", {}];
}
