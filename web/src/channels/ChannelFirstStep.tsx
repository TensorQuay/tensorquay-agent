"use client";

import "./catalogue";
import { useId } from "react";

import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";

/** A new channel's first step (WEB-U-063): what to tell its agents, and a first message to start from. */
export function ChannelFirstStep({
  purpose,
  onUse,
}: {
  purpose: string;
  onUse: (message: string) => void;
}) {
  const t = useT();
  const heading = useId();
  const suggestion = t("channels.firstMessage", { purpose });
  return (
    <section className="channel-first stack" aria-labelledby={heading}>
      <h2 id={heading} className="type-body-strong">
        {t("channels.firstTitle")}
      </h2>
      <p className="type-small muted">{t("channels.firstBody")}</p>
      <blockquote className="channel-first-message">{suggestion}</blockquote>
      <div>
        <Button variant="secondary" onClick={() => onUse(suggestion)}>
          {t("channels.firstUse")}
        </Button>
      </div>
    </section>
  );
}
