"use client";

import { useEffect, useState } from "react";

import { useT } from "../i18n/I18nProvider";

const MINUTE_MS = 60_000;
const HOUR_MINUTES = 60;
const DAY_MINUTES = 1440;

/** Refreshes relative activity time each minute in the reader's language and shows the absolute time on hover. */
export function RelativeTime({ value }: { value: string }) {
  const t = useT();
  const [now, setNow] = useState<number | null>(null);
  useEffect(() => {
    const refresh = () => setNow(Date.now());
    const first = requestAnimationFrame(refresh);
    const timer = setInterval(refresh, MINUTE_MS);
    return () => {
      cancelAnimationFrame(first);
      clearInterval(timer);
    };
  }, []);
  const minutes =
    now === null
      ? 0
      : Math.max(0, Math.floor((now - Date.parse(value)) / MINUTE_MS));
  const relative = new Intl.RelativeTimeFormat(t.locale, {
    numeric: "always",
    style: "short",
  });
  const label =
    minutes < 1
      ? t("time.justNow")
      : minutes < HOUR_MINUTES
        ? relative.format(-minutes, "minute")
        : minutes < DAY_MINUTES
          ? relative.format(-Math.floor(minutes / HOUR_MINUTES), "hour")
          : relative.format(-Math.floor(minutes / DAY_MINUTES), "day");
  const absolute = new Intl.DateTimeFormat(t.locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
  return (
    <time dateTime={value} title={absolute}>
      {now === null ? "…" : label}
    </time>
  );
}
