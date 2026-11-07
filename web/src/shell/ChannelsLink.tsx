"use client";

import { MessagesSquare } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";

import { useT } from "../i18n/I18nProvider";
import { Icon } from "../ui/Icon";

/** The Channels row in the sidebar, beside Workspaces (ADR-0014, UI-GUIDELINES §6); the icon alone on the rail. */
export function ChannelsLink({ onNavigate }: { onNavigate?: () => void }) {
  const pathname = usePathname();
  const t = useT();
  const current = pathname === "/channels" || pathname.startsWith("/channels/");
  return (
    <Link
      className="nav-item"
      href="/channels"
      prefetch={false}
      aria-label={t("channels.title")}
      title={t("channels.title")}
      aria-current={current ? "page" : undefined}
      onNavigate={() => onNavigate?.()}
    >
      <Icon glyph={MessagesSquare} />
      <span className="nav-label">{t("channels.title")}</span>
    </Link>
  );
}
