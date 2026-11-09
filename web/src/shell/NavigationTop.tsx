"use client";

import { SquarePen } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";

import { useConversationStart } from "../conversations/StartProvider";
import { useT } from "../i18n/I18nProvider";
import { Icon } from "../ui/Icon";

import { ChannelsLink } from "./ChannelsLink";
import { useNavTip } from "./nav-tip";
import { WorkspacesLink } from "./WorkspacesLink";

/**
 * The sidebar's top block (UI-GUIDELINES §6; the owner, 9 Nov 2026): New task, Workspaces and Channels, in that order
 * and one row style, before the task groups; the same in the phone's drawer. Each is named by its label, or on the
 * rail by the app's own tooltip.
 */
export function NavigationTop({
  onNavigate,
}: {
  onNavigate?: (() => void) | undefined;
}) {
  const pathname = usePathname();
  const { start } = useConversationStart();
  const t = useT();
  const { handlers, tip } = useNavTip(t("shell.newTask"));
  return (
    <div className="nav-items">
      <Link
        className="nav-item"
        href="/"
        prefetch={false}
        aria-label={t("shell.newTask")}
        onNavigate={() => {
          start();
          onNavigate?.();
        }}
        aria-current={pathname === "/" ? "page" : undefined}
        {...handlers}
      >
        <Icon glyph={SquarePen} />
        <span className="nav-label" data-nav-label="">
          {t("shell.newTask")}
        </span>
      </Link>
      {tip}
      <WorkspacesLink onNavigate={onNavigate} />
      <ChannelsLink onNavigate={onNavigate} />
    </div>
  );
}
