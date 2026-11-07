"use client";

import { lazy, Suspense, useRef, useState } from "react";

import { useT } from "../i18n/I18nProvider";
import { PanelBoundary } from "../ui/PanelBoundary";
import { Popover } from "../ui/Popover";

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
  const t = useT();
  return (
    <>
      <button
        type="button"
        ref={anchor}
        className="account-row"
        aria-label={t("account.label")}
        title={email || t("account.label")}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
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
        <span className="account-email type-small">{email}</span>
      </button>
      {open && (
        <Popover
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
        </Popover>
      )}
    </>
  );
}
