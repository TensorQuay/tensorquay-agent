"use client";

import { AccountMenu } from "../auth/AccountMenu";
import { useT } from "../i18n/I18nProvider";
import { Dialog } from "../ui/Dialog";

import { ChannelsLink } from "./ChannelsLink";
import { Navigation } from "./Navigation";
import { WorkspacesLink } from "./WorkspacesLink";
import "../i18n/deferred-catalogue";

/**
 * Loads the mobile navigation only when requested and closes after following a link. Below 768 px the sidebar is
 * hidden, so the drawer also holds the account menu at its foot, as the sidebar does (WEB-U-379): an item that
 * navigates (Settings, Sign out) closes the drawer too.
 */
export function NavigationDrawer({ onClose }: { onClose: () => void }) {
  const t = useT();
  return (
    <Dialog
      open
      onClose={onClose}
      title={t("shell.navigationDrawer")}
      className="navigation-drawer"
      placement="start"
    >
      <Navigation onNavigate={onClose} />
      <WorkspacesLink onNavigate={onClose} />
      <ChannelsLink onNavigate={onClose} />
      <div className="sidebar-account drawer-account">
        <AccountMenu onNavigate={onClose} />
      </div>
    </Dialog>
  );
}
