import type { MessageKey } from "../i18n/translate";

/**
 * Which run failures have their own plain words, and the words: data only, with no catalogue loaded, so a browser spec
 * can check the page against it (failure-copy.ts words them in the reader's language).
 */
/** What happened, and what the person can do about it: every failure reads as both (the owner's Mac test). */
export type Failure = { reason: MessageKey; next: MessageKey };

/**
 * Every failure code a run can stop with, in plain words. The runtime's recorded reason is a diagnostic, never shown;
 * failure-copy.test.tsx fails when the protocol gains a code without words here.
 */
const FAILURE_COPY: Readonly<Record<string, Failure>> = {
  provider_error: { reason: "run.providerError", next: "fail.providerNext" },
  budget: { reason: "run.budget", next: "fail.continueNext" },
  reply_cut: { reason: "run.replyCut", next: "fail.replyCutNext" },
  repeated_failure: { reason: "fail.repeated", next: "fail.repeatedNext" },
  no_progress: { reason: "fail.noProgress", next: "fail.noProgressNext" },
  fallback_parse: {
    reason: "run.fallbackParse",
    next: "fail.againOrModelNext",
  },
  io: { reason: "fail.io", next: "fail.againNext" },
  internal: { reason: "fail.internal", next: "fail.internalNext" },
  sandbox_failed: { reason: "fail.sandbox", next: "fail.sandboxNext" },
  // The same story as its "Not sent" notice (failure.agent).
  runtime_start_failed: {
    reason: "fail.runtimeStart",
    next: "fail.runtimeStartNext",
  },
  action_failed: { reason: "fail.action", next: "fail.againNext" },
  workspace_quota: {
    reason: "fail.workspaceQuota",
    next: "fail.workspaceQuotaNext",
  },
  state_quota: { reason: "fail.stateQuota", next: "fail.stateQuotaNext" },
  decision_stop: { reason: "fail.decisionStop", next: "fail.continueNext" },
  decision_engine_unavailable: {
    reason: "fail.decisionUnavailable",
    next: "fail.decisionRestoreNext",
  },
  decision_engine_budget_spent: {
    reason: "fail.decisionBudget",
    next: "fail.adminRaiseNext",
  },
  approvals_removed: {
    reason: "fail.approvalsRemoved",
    next: "fail.continueNext",
  },
  egress_denied: { reason: "fail.egress", next: "fail.egressNext" },
  host_keys_unsupported: {
    reason: "fail.hostKeysUnsupported",
    next: "fail.hostKeysUnsupportedNext",
  },
  busy: { reason: "fail.busy", next: "fail.againNext" },
  restarting: { reason: "fail.restarting", next: "fail.againNext" },
  store_unavailable: { reason: "fail.store", next: "fail.againNext" },
  permission_binding: { reason: "fail.protocol", next: "fail.protocolNext" },
  unknown_frame: { reason: "fail.protocol", next: "fail.protocolNext" },
  invalid_frame: { reason: "fail.protocol", next: "fail.protocolNext" },
  seq_gap: { reason: "fail.protocol", next: "fail.protocolNext" },
  unsupported_protocol: { reason: "fail.protocol", next: "fail.protocolNext" },
};

/**
 * A reply cut while the model was still thinking: its whole output limit went on reasoning, with no answer and no
 * call. The runtime's reason starts with these words (CORE-U-072), as a budget stop's names its limit (WEB-U-109).
 */
const THINKING_CUT = "the model spent its whole output limit thinking";
const THINKING: Failure = {
  reason: "run.thinkingCut",
  next: "fail.thinkingCutNext",
};

/**
 * A model that answered with nothing, every try (the runtime's `empty_response`, CORE-U): nothing refused the request,
 * so it isn't worded as a refusal (WEB-U-364, the 16e first hour). The runtime's reason starts with the code.
 */
/**
 * A lead's or helper's run ended by its agent team's spent budget (the gateway's `team_budget`, which the runtime ends
 * as `budget` with the reason "gateway.team_budget 409"): the team's budget, as its end note says (WEB-U-374).
 */
const TEAM_BUDGET: Failure = {
  reason: "run.teamBudget",
  next: "fail.teamBudgetNext",
};

/**
 * A helper stopped at its team's helpers' share of the budget (the gateway's `team_budget_helpers`, GW-I-304): its own
 * page says what happened to it and who writes up, never the lead's "carry on without them" (WEB-U-394, real-world use
 * before 17.0).
 */
const TEAM_BUDGET_HELPER: Failure = {
  reason: "fail.teamBudgetHelper",
  next: "fail.teamBudgetHelperNext",
};

const EMPTY_REPLY: Failure = {
  reason: "run.emptyReply",
  next: "fail.emptyReplyNext",
};

/** A budget stop by which limit it hit: the runtime's reason names it (tqa-core runner). */
const BUDGET_COPY: Readonly<Record<string, Failure>> = {
  "budget.output_tokens": {
    reason: "fail.outputTokens",
    next: "fail.outputTokensNext",
  },
  "budget.wall_clock": { reason: "fail.wallClock", next: "fail.continueNext" },
  "the run's step budget was reached": {
    reason: "fail.steps",
    next: "fail.continueNext",
  },
  "context window exceeded": {
    reason: "fail.contextWindow",
    next: "fail.contextWindowNext",
  },
};

/**
 * A model call the gateway refused by itself (GW-I-147): its relay marks the refusal (X-TQA-Gateway-Error), and the
 * runtime puts the code first in the run's reason as "gateway.<code> <status>". The provider said nothing, so the
 * gateway's words replace "The provider said". Every code the model relay answers by itself, from the gateway's
 * relay (endpoint, request, rate, throttle, forward).
 */
const GATEWAY_COPY: Readonly<Record<string, Failure>> = {
  unauthorized: {
    reason: "fail.gwUnauthorized",
    next: "fail.gwUnauthorizedNext",
  },
  forbidden: { reason: "fail.gwForbidden", next: "fail.gwForbiddenNext" },
  not_found: { reason: "fail.gwNotFound", next: "fail.gwNotFoundNext" },
  invalid_request: { reason: "fail.gwInvalid", next: "fail.internalNext" },
  body_too_large: {
    reason: "fail.contextWindow",
    next: "fail.contextWindowNext",
  },
  rate_limited: {
    reason: "fail.gwRateLimited",
    next: "fail.gwRateLimitedNext",
  },
  restarting: { reason: "fail.restarting", next: "fail.againNext" },
  store_unavailable: { reason: "fail.gwStore", next: "fail.againNext" },
  secret_unavailable: { reason: "fail.gwSecret", next: "fail.gwSecretNext" },
  key_use_unrecorded: {
    reason: "fail.keyUnrecorded",
    next: "fail.keyUnrecordedNext",
  },
  upstream_unavailable: {
    reason: "fail.gwUpstream",
    next: "fail.gwUpstreamNext",
  },
  timeout: { reason: "fail.gwTimeout", next: "fail.gwUpstreamNext" },
  configuration: {
    reason: "fail.gwConfiguration",
    next: "fail.gwConfigurationNext",
  },
};
/** A gateway code this page has no words for: still our server's refusal, never the provider's. */
const GATEWAY_UNKNOWN: Failure = {
  reason: "fail.gwRefused",
  next: "fail.internalNext",
};
/** The gateway's own code, when the gateway refused the call itself ("gateway.<code> <status>"). */
export const gatewayCode = (reason: string) => {
  const first = reason.split(" ")[0] ?? "";
  return first.startsWith("gateway.") ? first.slice("gateway.".length) : null;
};

/**
 * A model server refusing input longer than its context. Its words vary by server (vLLM, llama.cpp, Ollama, OpenAI,
 * Anthropic); they lead the outcome as ever (WEB-U-055), and the next step says what helps.
 */
const TOO_LONG =
  /context (length|window|size)|too long|too many tokens|exceeds? (the )?(max|maximum|context)|maximum.*tokens/i;

/** The plain words for a failure, or null for a code this web doesn't know. */
export function knownFailure(
  code: string,
  reason: string,
  provider?: string | null,
): Failure | null {
  const gateway = code === "provider_error" ? gatewayCode(reason) : null;
  if (gateway) return GATEWAY_COPY[gateway] ?? GATEWAY_UNKNOWN;
  if (code === "provider_error" && provider && TOO_LONG.test(provider))
    return BUDGET_COPY["context window exceeded"] ?? null;
  if (code === "budget" && gatewayCode(reason) === "team_budget")
    return TEAM_BUDGET;
  if (code === "budget" && gatewayCode(reason) === "team_budget_helpers")
    return TEAM_BUDGET_HELPER;
  if (code === "budget")
    return BUDGET_COPY[reason] ?? FAILURE_COPY.budget ?? null;
  if (code === "reply_cut" && reason.startsWith(THINKING_CUT)) return THINKING;
  if (code === "provider_error" && /^empty_response(:|$)/.test(reason))
    return EMPTY_REPLY;
  return FAILURE_COPY[code] ?? null;
}

/** Whether the page has words for a gateway code (failureCode shows the code of one it doesn't). */
export const gatewayKnown = (code: string) => code in GATEWAY_COPY;
