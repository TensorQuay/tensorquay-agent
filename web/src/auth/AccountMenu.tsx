"use client";

import { lazy, Suspense, useId, useRef, useState } from "react";

import { useT } from "../i18n/I18nProvider";
import { useNavTip } from "../shell/nav-tip";
import { prefetchMenus, useMenus } from "../ui/lazy-menus";
import { PanelBoundary } from "../ui/PanelBoundary";

import { initials } from "./initials";
import { useSession } from "./use-session";

const AccountActions = lazy(() =>
  import("./AccountActions").then(({ AccountActions }) => ({
    default: AccountActions,
  })),
);

/**
 * The account row at the bottom of the sidebar, and of the phone's navigation drawer (UI-GUIDELINES §7); its menu opens
 * upward and loads on demand. `onNavigate` hears an item that leaves the page (Settings, Sign out): the drawer closes.
 */
export function AccountMenu({ onNavigate }: { onNavigate?: () => void } = {}) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const email = useSession().data?.email ?? "";
  // Which account is signed in, to a screen reader too: the email describes the button (its name is "Account").
  const signedIn = useId();
  const t = useT();
  // The email names the row; where it is hidden (the rail) or cut, the app's own tooltip shows it whole (§6).
  const { handlers, tip } = useNavTip(email || t("account.label"));
  // Its menu's frame comes when it is about to open (WEB-U-014's budget); a menu that can't get it closes.
  const menus = useMenus(open, setOpen, anchor);
  return (
    <>
      <button
        type="button"
        ref={anchor}
        className="account-row"
        aria-label={t("account.label")}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-describedby={signedIn}
        onClick={() => setOpen(!open)}
        {...handlers}
        onPointerEnter={(event) => {
          prefetchMenus();
          handlers.onPointerEnter(event);
        }}
        onFocus={(event) => {
          prefetchMenus();
          handlers.onFocus(event);
        }}
        onKeyDown={(event) => {
          if (["ArrowDown", "ArrowUp"].includes(event.key)) {
            event.preventDefault();
            setOpen(true);
          }
        }}
      >
        <span className="account-avatar type-label" aria-hidden="true">
          {initials(email)}
        </span>
        <span
          className="account-email type-small"
          id={signedIn}
          data-nav-label=""
        >
          {email}
        </span>
      </button>
      {tip}
      {open && menus && (
        <menus.Popover
          anchor={anchor}
          label={t("account.actions")}
          placement="above-start"
          onClose={() => setOpen(false)}
        >
          <PanelBoundary>
            <Suspense
              fallback={
                <span role="status" className="type-small muted">
                  {t("account.opening")}
                </span>
              }
            >
              <AccountActions
                onClose={() => setOpen(false)}
                onNavigate={() => {
                  setOpen(false);
                  onNavigate?.();
                }}
              />
            </Suspense>
          </PanelBoundary>
        </menus.Popover>
      )}
    </>
  );
}
