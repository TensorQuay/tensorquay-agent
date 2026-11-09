"use client";

import { Folders } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";

import { useT } from "../i18n/I18nProvider";
import { Icon } from "../ui/Icon";

import { useNavTip } from "./nav-tip";

/** The Workspaces row in the sidebar's top block, after New task; the icon alone on the rail, named by its tip. */
export function WorkspacesLink({
  onNavigate,
}: {
  onNavigate?: (() => void) | undefined;
}) {
  const pathname = usePathname();
  const t = useT();
  const { handlers, tip } = useNavTip(t("workspaces.title"));
  return (
    <>
      <Link
        className="nav-item"
        href="/workspaces"
        prefetch={false}
        aria-label={t("workspaces.title")}
        aria-current={pathname === "/workspaces" ? "page" : undefined}
        onNavigate={() => onNavigate?.()}
        {...handlers}
      >
        <Icon glyph={Folders} />
        <span className="nav-label" data-nav-label="">
          {t("workspaces.title")}
        </span>
      </Link>
      {tip}
    </>
  );
}
