"use client";

import Link from "next/link";
import { lazy, Suspense, useState, type ReactNode } from "react";

import { AccountMenu } from "../auth/AccountMenu";
import { TaskSearchHost } from "../conversations/TaskSearchHost";
import { useT } from "../i18n/I18nProvider";
import { APP_NAME } from "../lib/app";
import { Skeleton } from "../ui/Skeleton";

import { useNavTip } from "./nav-tip";
import { Navigation } from "./Navigation";
import { ShellContext } from "./shell-context";
import { SidebarToggle } from "./SidebarToggle";

const NavigationDrawer = lazy(() =>
  import("./NavigationDrawer").then(({ NavigationDrawer }) => ({
    default: NavigationDrawer,
  })),
);

/** The app shell without a global header (UI-GUIDELINES §6): a sidebar or rail, and the content column. */
export function Shell({ children }: { children: ReactNode }) {
  const [navigationOpen, setNavigationOpen] = useState(false);
  const t = useT();
  const brand = useNavTip(APP_NAME);
  return (
    <ShellContext.Provider
      value={{ openNavigation: () => setNavigationOpen(true) }}
    >
      <div className="app-shell">
        <aside className="app-sidebar" aria-label={t("shell.sidebar")}>
          <div className="sidebar-brand">
            <Link
              href="/"
              prefetch={false}
              className="product-name"
              aria-label={APP_NAME}
              {...brand.handlers}
            >
              <span className="brand-mark" aria-hidden="true" />
              <span className="product-label type-body" data-nav-label="">
                {APP_NAME}
              </span>
            </Link>
            {brand.tip}
            <SidebarToggle />
          </div>
          <Navigation />
          <div className="sidebar-account">
            <AccountMenu />
          </div>
        </aside>
        <main id="main" className="app-content">
          {children}
        </main>
        {/* The one task search and its Ctrl/Cmd K; a match opened closes the drawer too. */}
        <TaskSearchHost onNavigate={() => setNavigationOpen(false)} />
        {navigationOpen && (
          <Suspense
            fallback={<Skeleton label={t("shell.openingNavigation")} />}
          >
            <NavigationDrawer onClose={() => setNavigationOpen(false)} />
          </Suspense>
        )}
      </div>
    </ShellContext.Provider>
  );
}
