"use client";

import "./catalogue";
import Link from "next/link";

import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import { APP_NAME } from "../lib/app";
import { NavigationButton } from "../shell/NavigationButton";
import { TabTitle } from "../shell/TabTitle";
import { Button } from "../ui/Button";

/**
 * A channel that couldn't be opened, as a task that couldn't is (UI-GUIDELINES §14: a visible message with a next step):
 * the page's bar keeps its place, and the message is centred under it with a way on. One that doesn't exist, or that
 * the reader isn't in (404, or 403), says so in the same words and leads back to the channels; trying again wouldn't
 * help. Any other failure offers Try again. Before, a bare grey line stood where the bar's title goes (a pass over the
 * pages before 17.0; WEB-U-381).
 */
export function ChannelMissing({
  error,
  retry,
}: {
  error: unknown;
  retry: () => void;
}) {
  const t = useT();
  const gone =
    error instanceof ApiError && (error.status === 404 || error.status === 403);
  return (
    <div className="channel-missing">
      <TabTitle>{t("channels.tab", { product: APP_NAME })}</TabTitle>
      <section className="channel-bar" aria-label={t("channels.header")}>
        <NavigationButton />
        <h1 className="type-heading">{t("channels.title")}</h1>
      </section>
      <div className="conversation-failure stack">
        <p role="alert">
          {t(gone ? "channels.notFound" : "channels.openFailed")}
        </p>
        <div className="conversation-failure-actions">
          {gone ? (
            <Link href="/channels" className="button" data-variant="primary">
              {t("channels.goToChannels")}
            </Link>
          ) : (
            <Button variant="secondary" onClick={retry}>
              {t("common.tryAgain")}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
