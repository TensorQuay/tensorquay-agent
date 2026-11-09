"use client";

import { ConversationList } from "../conversations/ConversationList";
import { useT } from "../i18n/I18nProvider";
import { PanelBoundary } from "../ui/PanelBoundary";

import { NavigationTop } from "./NavigationTop";

/** The sidebar's navigation: its top block (New task, Workspaces, Channels), then the user's task list. */
export function Navigation({ onNavigate }: { onNavigate?: () => void }) {
  const t = useT();
  return (
    <nav className="navigation" aria-label={t("shell.navigation")}>
      <NavigationTop onNavigate={onNavigate} />
      <PanelBoundary>
        <ConversationList onNavigate={onNavigate} />
      </PanelBoundary>
    </nav>
  );
}
