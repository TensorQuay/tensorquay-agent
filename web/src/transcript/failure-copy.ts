import { english, type Translate } from "../i18n/translate";

import { gatewayCode, gatewayKnown, knownFailure } from "./failure-keys";
import { message, type Copy } from "./notice-copy";

export { gatewayCode, knownFailure };
type Run =
  | {
      code: string | null;
      reason: string | null;
      providerMessage?: string | null;
    }
  | undefined;

/**
 * A failed run's secondary code line: the provider's classification and status ("http 400", UX review 0927b item 4),
 * or a code this web has no words for (the gateway's too), after its plain sentence. Null when the words say it all.
 */
export function failureCode(run: {
  code: string | null;
  reason: string | null;
}): string | null {
  if (!run.code) return null;
  if (run.code === "provider_error") {
    // Our server's refusal: its words say it all, and a code it has no words for follows them.
    const gateway = gatewayCode(run.reason ?? "");
    if (gateway === null) return run.reason;
    return gatewayKnown(gateway) ? null : gateway;
  }
  return knownFailure(run.code, run.reason ?? "") ? null : run.code;
}

/** A failed run's reason as notice copy: its plain words, never the recorded diagnostic. */
export function failureCopy(
  code: string,
  reason: string,
  provider?: string | null,
): Copy {
  return message(
    knownFailure(code, reason, provider)?.reason ?? "run.stoppedUnknown",
  );
}

/** What happened, in the reader's language; "The run stopped" for a code without words, null with no code at all. */
export function failureReason(run: Run, t: Translate = english): string | null {
  if (!run?.code) return null;
  return t(
    knownFailure(run.code, run.reason ?? "", run.providerMessage)?.reason ??
      "run.stoppedUnknown",
  );
}

/** What the person can do next about a failed run. */
export function failureNext(run: Run, t: Translate = english): string {
  const known = run?.code
    ? knownFailure(run.code, run.reason ?? "", run.providerMessage)
    : null;
  return t(known?.next ?? "summary.failedNext");
}

/** The most of a decision engine's reason the page shows; the runtime already bounds it (ADR-0013). */
export const ENGINE_WORDS_MAX = 200;

/**
 * A decision engine's own reason for stopping the run (ADR-0013's `Stop { reason }`, WEB-U-328), bounded, for after
 * the plain sentence; none for any other failure, whose recorded reason stays a diagnostic, or an empty reason.
 */
export function failureEngine(run: {
  code: string | null;
  reason: string | null;
}): string | null {
  if (run.code !== "decision_stop") return null;
  const words = (run.reason ?? "").trim();
  if (!words) return null;
  const letters = [...words];
  return letters.length > ENGINE_WORDS_MAX
    ? `${letters.slice(0, ENGINE_WORDS_MAX).join("")}…`
    : words;
}

/**
 * The provider's own words for a failed run, which lead its outcome (WEB-U-055); none when the gateway refused the
 * call itself, since the provider said nothing.
 */
export function failureProvider(run: {
  code: string | null;
  reason: string | null;
  providerMessage?: string | null;
}): string | null {
  if (run.code === "provider_error" && gatewayCode(run.reason ?? ""))
    return null;
  return run.providerMessage ?? null;
}
