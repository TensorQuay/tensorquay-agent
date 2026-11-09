"use client";

import { MessagesSquare } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";

import { useT } from "../i18n/I18nProvider";
import { Icon } from "../ui/Icon";

import { useNavTip } from "./nav-tip";

/** The Channels row in the sidebar's top block, after Workspaces (ADR-0014, UI-GUIDELINES §6); the icon alone on the
 * rail, named by its tip. */
export function ChannelsLink({
  onNavigate,
}: {
  onNavigate?: (() => void) | undefined;
}) {
  const pathname = usePathname();
  const t = useT();
  const { handlers, tip } = useNavTip(t("channels.title"));
  const current = pathname === "/channels" || pathname.startsWith("/channels/");
  return (
    <>
      <Link
        className="nav-item"
        href="/channels"
        prefetch={false}
        aria-label={t("channels.title")}
        aria-current={current ? "page" : undefined}
        onNavigate={() => onNavigate?.()}
        {...handlers}
      >
        <Icon glyph={MessagesSquare} />
        <span className="nav-label" data-nav-label="">
          {t("channels.title")}
        </span>
      </Link>
      {tip}
    </>
  );
}
