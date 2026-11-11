"use client";

import {
  Activity,
  BarChart3,
  Bot,
  Box,
  Gauge,
  Globe,
  KeyRound,
  LayoutDashboard,
  Plug,
  Route,
  Sparkles,
  Terminal,
  User as UserIcon,
  Users,
  type LucideIcon,
} from "lucide-react";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { type ReactNode, useLayoutEffect, useRef } from "react";

import { useSession } from "../auth/use-session";
import { useT } from "../i18n/I18nProvider";
import { APP_NAME } from "../lib/app";
import type { User } from "../protocol/generated/api";
import { NavigationButton } from "../shell/NavigationButton";
import { TabTitle } from "../shell/TabTitle";
import { Button } from "../ui/Button";
import { ErrorMessage } from "../ui/ErrorMessage";
import { Icon } from "../ui/Icon";
import { MenuButton } from "../ui/MenuButton";
import { Skeleton } from "../ui/Skeleton";

import { GROUPS, SECTIONS, type SettingsSection } from "./sections";
import { SettingsCatalogue } from "./SettingsCatalogue";

export type { SettingsSection } from "./sections";

/** A line icon before each section's name, as in the main sidebar (UI-GUIDELINES §5c); decorative only. */
const ICONS: Readonly<Record<SettingsSection, LucideIcon>> = {
  account: UserIcon,
  tokens: Terminal,
  keys: KeyRound,
  profiles: Bot,
  overview: LayoutDashboard,
  people: Users,
  models: Sparkles,
  environments: Box,
  connectors: Plug,
  decisionEngines: Route,
  internetAccess: Globe,
  limits: Gauge,
  usage: BarChart3,
  status: Activity,
};

/** The section's heading, by which a section's main list may be named (People's table). */
export const SECTION_HEADING = "settings-section-heading";

/**
 * Settings (UI-GUIDELINES §6, WEB-U-075): a page heading, the sections on the left in three groups on wide screens (one
 * grouped Section select above the column below 768 px) and a 720 px column. Your account is everyone's;
 * Administration and Operations are for administrators only.
 */
export function SettingsShell(props: {
  section: SettingsSection;
  children: (me: User) => ReactNode;
}) {
  return (
    <SettingsCatalogue>
      <Shell {...props} />
    </SettingsCatalogue>
  );
}

/** Below 768 px the Section select stands in for the menu, which isn't drawn (settings.css). */
const NARROW = "(max-width: 767px)";

/**
 * Scrolls `menu`, and only `menu`, so its current item is inside the part of it the window shows: its box clipped to
 * the window (WEB-U-460). `scrollIntoView` would scroll the page too, since the sticky menu's foot sits below the window
 * while the page is at its top. The menu's padding stays around the item, room for its focus ring. A menu that isn't
 * drawn (below 768 px, where the Section select stands in) has no box and is left alone.
 */
function showCurrent(menu: HTMLElement) {
  const item = menu.querySelector('[aria-current="page"]');
  const box = menu.getBoundingClientRect();
  if (!item || box.height === 0) return;
  const top = Math.max(box.top, 0);
  const bottom = Math.min(box.bottom, window.innerHeight);
  const room = Number.parseFloat(getComputedStyle(menu).paddingTop) || 0;
  const place = item.getBoundingClientRect();
  if (place.top - room < top) menu.scrollTop -= top - place.top + room;
  else if (place.bottom + room > bottom)
    menu.scrollTop += place.bottom + room - bottom;
}

/** The shell itself, rendered once the settings copy has loaded. */
function Shell({
  section,
  children,
}: {
  section: SettingsSection;
  children: (me: User) => ReactNode;
}) {
  const t = useT();
  const router = useRouter();
  const pathname = usePathname();
  const session = useSession();
  const admin = session.data?.role === "admin";
  const shown = SECTIONS.filter((entry) => admin || !entry.admin);
  const groups = GROUPS.map((group) => ({
    ...group,
    entries: shown.filter((entry) => entry.group === group.id),
  })).filter((group) => group.entries.length > 0);
  const current = SECTIONS.find((entry) => entry.id === section);
  const title = current ? t(current.label) : t("settings.title");
  // WEB-U-460: a menu taller than the window scrolls on its own. Whenever the current section's item appears (an
  // administrator's arrives with the session, after the first render), the menu scrolls to show it, and only the menu.
  // It does so before the page is painted, so the menu never moves once it shows; then again whenever a wider window
  // draws the menu, and once the fonts have loaded, which can move the item.
  const menu = useRef<HTMLElement>(null);
  const listed = shown.some((entry) => entry.id === section);
  useLayoutEffect(() => {
    const node = menu.current;
    if (!listed || !node) return;
    const show = () => showCurrent(node);
    show();
    const narrow = typeof matchMedia === "function" ? matchMedia(NARROW) : null;
    narrow?.addEventListener("change", show);
    let live = true;
    if ("fonts" in document)
      void document.fonts.ready.then(() => {
        if (live) show();
      });
    return () => {
      live = false;
      narrow?.removeEventListener("change", show);
    };
  }, [section, listed]);
  return (
    <div className="settings-page">
      <TabTitle>
        {t("settings.tab", { section: title, product: APP_NAME })}
      </TabTitle>
      <div className="settings-title">
        <NavigationButton />
        <h1 className="type-heading">{t("settings.title")}</h1>
      </div>
      <nav
        ref={menu}
        className="settings-nav"
        aria-label={t("settings.sections")}
      >
        {groups.map((group) => (
          <div
            key={group.id}
            role="group"
            aria-label={t(group.label)}
            className="settings-group"
          >
            <h2 className="type-label muted settings-group-label">
              {t(group.label)}
            </h2>
            {group.entries.map((entry) => (
              <Link
                key={entry.id}
                href={entry.href}
                className="nav-item"
                aria-current={entry.id === section ? "page" : undefined}
              >
                <Icon glyph={ICONS[entry.id]} className="nav-icon" />
                {t(entry.label)}
              </Link>
            ))}
          </div>
        ))}
      </nav>
      {/* Small screens: the same sections in the standard picker's grouped menu, full width (WEB-U-134). */}
      <div className="settings-select">
        <span className="type-label muted" aria-hidden="true">
          {t("settings.section")}
        </span>
        <MenuButton
          label={t("settings.section")}
          field
          value={current?.href ?? ""}
          placeholder={t("settings.title")}
          options={groups.flatMap((group) =>
            group.entries.map((entry) => ({
              id: entry.href,
              label: t(entry.label),
              group: t(group.label),
            })),
          )}
          onChange={(href) => router.push(href)}
        />
      </div>
      <div className="settings-column">
        {/* Every section opens with its name (17.3); a page inside it keeps its own title, under its way back. */}
        {current &&
          (pathname === current.href ||
            ("views" in current &&
              current.views.some((view) => view === pathname))) && (
            <h2
              id={SECTION_HEADING}
              className="type-heading settings-section-title"
            >
              {title}
            </h2>
          )}
        {session.isError ? (
          <div className="stack">
            <ErrorMessage error={session.error} />
            <Button
              variant="secondary"
              onClick={() => {
                void session.refetch();
              }}
            >
              {t("common.tryAgain")}
            </Button>
          </div>
        ) : session.data ? (
          children(session.data)
        ) : (
          <Skeleton label={t("skeleton.loading")} />
        )}
      </div>
    </div>
  );
}
