"use client";

import { useT } from "../i18n/I18nProvider";

/** A calendar date in the reader's language (future dates such as expiry, where "ago" does not apply). */
export function DateLabel({ value }: { value: string }) {
  const t = useT();
  const date = new Date(value);
  return (
    <time
      dateTime={value}
      title={new Intl.DateTimeFormat(t.locale, {
        dateStyle: "long",
        timeStyle: "short",
      }).format(date)}
    >
      {new Intl.DateTimeFormat(t.locale, { dateStyle: "medium" }).format(date)}
    </time>
  );
}
