"use client";

import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";

/** Error codes with their own copy; any other code gets the generic sentence, never the gateway's diagnostic. */
const COPY: Readonly<Record<string, MessageKey>> = {
  network: "error.network",
  timeout: "error.timeout",
  cancelled: "error.cancelled",
  invalid_response: "error.invalid_response",
  randomness_unavailable: "error.randomness_unavailable",
  request_failed: "error.request_failed",
  rate_limited: "error.rate_limited",
  // GW-I-147: the gateway uses no key it can't record the use of.
  key_use_unrecorded: "error.key_use_unrecorded",
  copy_failed: "error.copy_failed",
};

/** Announces an actionable error once, in the reader's language, with the gateway's retry delay when supplied. */
export function ErrorMessage({ error }: { error: Error | string | null }) {
  const t = useT();
  if (!error) return null;
  const message =
    typeof error === "string"
      ? error
      : error instanceof ApiError
        ? // A server error reads as one, whatever its body held (finding 3).
          t(
            error.status >= 500 && error.code === "invalid_response"
              ? "error.server"
              : (COPY[error.code] ?? "error.generic"),
          )
        : error.message;
  const delay = error instanceof ApiError ? error.retryAfter : undefined;
  return (
    <p role="alert" className="error-message type-small">
      {message}
      {delay !== undefined
        ? ` ${t("error.retryAfter", { seconds: Math.ceil(delay) })}`
        : ""}
    </p>
  );
}
