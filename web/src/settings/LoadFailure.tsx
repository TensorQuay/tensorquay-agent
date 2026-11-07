"use client";

import { useEffect, useRef } from "react";

import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";

/** The place that holds what loads in the failure's stead: its section, region or main, else its parent. */
function holderOf(parent: HTMLElement | null): HTMLElement | null {
  return (
    parent?.closest<HTMLElement>("section, [role='region'], main") ?? parent
  );
}

/**
 * A section that could not load: one plain sentence and the action that retries it. Once a retry works the failure
 * gives way to the content, and Try again with it, so the focus goes to the place that now holds the content rather
 * than the page, unless the person has moved it meanwhile (UI-GUIDELINES §9; WEB-U-276).
 */
export function LoadFailure({
  message,
  onRetry,
}: {
  message: string;
  onRetry: () => void;
}) {
  const t = useT();
  const own = useRef<HTMLDivElement>(null);
  const retried = useRef(false);
  useEffect(() => {
    const parent = own.current?.parentElement ?? null;
    return () => {
      if (!retried.current) return;
      requestAnimationFrame(() => {
        const active = document.activeElement;
        if (active && active !== document.body) return;
        const target = holderOf(parent);
        if (!target?.isConnected) return;
        if (!target.hasAttribute("tabindex"))
          target.setAttribute("tabindex", "-1");
        target.focus();
      });
    };
  }, []);
  return (
    <div className="stack" ref={own}>
      <p>{message}</p>
      <div>
        <Button
          variant="secondary"
          onClick={() => {
            retried.current = true;
            onRetry();
          }}
        >
          {t("common.tryAgain")}
        </Button>
      </div>
    </div>
  );
}
