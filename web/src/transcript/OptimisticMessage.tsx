import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";

import { notSentMessage } from "./action-failure";

/** Keeps an unconfirmed prompt visible, including an explicit retry after failure. */
export function OptimisticMessage({
  text,
  failed = false,
  message,
  status = "sending",
  onRetry,
  onCancel,
}: {
  text: string;
  failed?: boolean;
  message?: string | undefined;
  status?: "sending" | "queued" | "starting";
  onRetry?: () => void;
  /** Takes back a message still waiting for a slot or for its sandbox (WEB-U-167). */
  onCancel?: () => void;
}) {
  const t = useT();
  return (
    <article className="user-message" aria-label={t("prompt.yours")}>
      <p className="message-text">{text}</p>
      {/* Waiting for a free slot is said once, in the header (WEB-U-161): the message adds no line of its own. */}
      {(failed || status !== "queued") && (
        <p
          className={failed ? "type-small muted" : "type-label muted"}
          role={failed ? "alert" : undefined}
        >
          {failed
            ? notSentMessage(message, t)
            : t(status === "starting" ? "prompt.starting" : "prompt.sending")}
        </p>
      )}
      {failed && onRetry && (
        <Button variant="ghost" onClick={onRetry}>
          {t("prompt.retry")}
        </Button>
      )}
      {!failed && onCancel && (
        <Button
          variant="ghost"
          className="message-cancel"
          aria-label={t("prompt.cancelName")}
          onClick={onCancel}
        >
          {t("common.cancel")}
        </Button>
      )}
    </article>
  );
}
