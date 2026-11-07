"use client";

import { Folders } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";

import { useT } from "../i18n/I18nProvider";
import { Icon } from "../ui/Icon";

/** The Workspaces row at the bottom of the sidebar, above the account row; the icon alone on the rail. */
export function WorkspacesLink({ onNavigate }: { onNavigate?: () => void }) {
  const pathname = usePathname();
  const t = useT();
  return (
    <Link
      className="nav-item"
      href="/workspaces"
      prefetch={false}
      aria-label={t("workspaces.title")}
      title={t("workspaces.title")}
      aria-current={pathname === "/workspaces" ? "page" : undefined}
      onNavigate={() => onNavigate?.()}
    >
      <Icon glyph={Folders} />
      <span className="nav-label">{t("workspaces.title")}</span>
    </Link>
  );
}
