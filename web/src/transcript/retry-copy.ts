import type { MessageKey } from "../i18n/translate";

/** The same, as a phrase inside the step's summary line ("the model was slow, retried twice"). */
export function retryShort(reason: string): MessageKey {
  const long = retryWords(reason);
  return `retryShort.${long.slice("retry.".length)}` as MessageKey;
}

/**
 * A turn's retries for a tooltip: each reason's words once, in order, then the runtime's codes once each, which only
 * follow a "Details:" label (WEB-U-304: a bare "fallback_parse: malformed" read as the explanation).
 */
export function retryDetails(reasons: string[]): {
  words: MessageKey[];
  codes: string[];
} {
  return {
    words: [...new Set(reasons.map(retryWords))],
    codes: [...new Set(reasons)],
  };
}

/**
 * Why a model request was retried, in the reader's words (UI-GUIDELINES §10): the runtime's code ("http 503",
 * "fallback_parse: schema") stays in a tooltip, after its words. A code this web doesn't know reads as a failed request.
 */
export function retryWords(reason: string): MessageKey {
  // The text route refused a reply that was only its own correction sentence, copied back: not unreadable, but the
  // model answering with our instruction (the runtime's exact reason; any other fallback_parse reads as unreadable).
  if (reason === "fallback_parse: echo") return "retry.echo";
  const code = reason.split(/[ :]/, 1)[0] ?? "";
  switch (code) {
    case "http":
      return "retry.http";
    case "rate_limited":
      return "retry.rateLimited";
    case "unavailable":
      return "retry.unavailable";
    case "timeout":
      return "retry.timeout";
    case "stream_cut":
      return "retry.streamCut";
    case "protocol":
    case "tool_args_invalid":
    case "tool_args_cut":
    case "fallback_parse":
      return "retry.unreadable";
    case "io":
      return "retry.unreachable";
    case "native_tools":
      return "retry.hiddenCall";
    case "gateway_link":
      return "retry.gatewayLink";
    // The model answered with nothing (WEB-U-364): not a failed request, and nothing refused it.
    case "empty_response":
      return "retry.empty";
    default:
      return "retry.failed";
  }
}
