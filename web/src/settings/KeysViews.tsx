"use client";

import Link from "next/link";

import { useT } from "../i18n/I18nProvider";

/** The switch between a person's own keys and the shared keys (WEB-U-076): administrators manage the shared ones, a
 * member reads those they may use (the Keys sketch §6). */
export function KeysViews({ current }: { current: "mine" | "shared" }) {
  const t = useT();
  const views = [
    { id: "mine", label: t("keys.mine"), href: "/settings/keys" },
    { id: "shared", label: t("keys.shared"), href: "/settings/keys/shared" },
  ] as const;
  return (
    <>
      {/* The section's one line, over both its views (17.3). */}
      <p className="type-small muted">{t("keys.sectionLine")}</p>
      {/* Links to two pages, so a navigation, drawn as the one tab style (UI-GUIDELINES §7 Tabs). */}
      <nav className="tab-list" aria-label={t("keys.views")}>
        {views.map((view) => (
          <Link
            key={view.id}
            href={view.href}
            className="tab"
            aria-current={view.id === current ? "page" : undefined}
          >
            {view.label}
          </Link>
        ))}
      </nav>
    </>
  );
}
