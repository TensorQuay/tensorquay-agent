"use client";

import { SquarePen } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";

import { ConversationList } from "../conversations/ConversationList";
import { useConversationStart } from "../conversations/StartProvider";
import { useT } from "../i18n/I18nProvider";
import { Icon } from "../ui/Icon";
import { PanelBoundary } from "../ui/PanelBoundary";

/** Shows only available navigation and the current user's conversation list. */
export function Navigation({ onNavigate }: { onNavigate?: () => void }) {
  const pathname = usePathname();
  const { start } = useConversationStart();
  const t = useT();
  return (
    <nav className="navigation" aria-label={t("shell.navigation")}>
      <Link
        className="nav-item"
        href="/"
        prefetch={false}
        aria-label={t("shell.newTask")}
        title={t("shell.newTask")}
        onNavigate={() => {
          start();
          onNavigate?.();
        }}
        aria-current={pathname === "/" ? "page" : undefined}
      >
        <Icon glyph={SquarePen} />
        <span className="nav-label">{t("shell.newTask")}</span>
      </Link>
      <PanelBoundary>
        <ConversationList onNavigate={onNavigate} />
      </PanelBoundary>
    </nav>
  );
}
