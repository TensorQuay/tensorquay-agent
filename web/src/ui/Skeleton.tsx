"use client";

import { useT } from "../i18n/I18nProvider";

/** An announced loading placeholder that respects reduced motion. */
export function Skeleton({
  label,
  lines = 3,
}: {
  label?: string;
  lines?: number;
}) {
  const t = useT();
  return (
    <div
      role="status"
      aria-label={label ?? t("skeleton.loading")}
      className="skeleton"
    >
      <span className="sr-only">{label ?? t("skeleton.loading")}</span>
      {Array.from({ length: lines }, (_, index) => (
        <span key={index} className="skeleton-line" aria-hidden="true" />
      ))}
    </div>
  );
}
