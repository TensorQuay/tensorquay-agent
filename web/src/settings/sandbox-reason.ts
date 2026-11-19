import type { Translate } from "../i18n/translate";

/**
 * Why a sweep couldn't remove something, in plain words (the Sandboxes page and Status): the gateway sends its error
 * code (`timeout`, `engine_unavailable`, `store_unavailable`, `sandbox_failed`); a code this page doesn't know yet
 * reads as the general refusal. Nothing for no reason.
 */
export function reasonWords(code: string | null | undefined, t: Translate) {
  return code ? t("sandboxes.reasonWords", { reason: code }) : "";
}
