"use client";

import { useT } from "../i18n/I18nProvider";

/** The keyboard shortcut past the sidebar to the page's main content. */
export function SkipLink() {
  const t = useT();
  return (
    <a href="#main" className="skip-link">
      {t("shell.skipToContent")}
    </a>
  );
}
