"use client";

import { PanelLeftClose, PanelLeftOpen } from "lucide-react";
import { useEffect, useSyncExternalStore } from "react";

import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";

import {
  currentSidebar,
  effectiveSidebar,
  railByWidth,
  setSidebar,
  subscribeSidebar,
  type SidebarState,
} from "./sidebar";

const serverState = (): SidebarState => "auto";
const serverWidth = () => false;
const noSubscription = () => () => undefined;

/** Whether a key press belongs to text the user is editing. */
function typing(target: EventTarget | null) {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable ||
      target.matches("input, textarea, select, [role='textbox']"))
  );
}

/** Switches between the expanded sidebar and the rail; the key [ does the same outside text fields. */
export function SidebarToggle() {
  const state = useSyncExternalStore(
    subscribeSidebar,
    currentSidebar,
    serverState,
  );
  const narrow = useSyncExternalStore(
    subscribeSidebar,
    railByWidth,
    serverWidth,
  );
  const expanded = effectiveSidebar(state, narrow) === "expanded";
  // Until hydrated the page can't know the width or the stored choice; the stylesheet already shows the layout.
  const known = useSyncExternalStore(
    noSubscription,
    () => true,
    () => false,
  );
  const t = useT();
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (
        event.key !== "[" ||
        event.ctrlKey ||
        event.metaKey ||
        event.altKey ||
        typing(event.target)
      )
        return;
      setSidebar(
        effectiveSidebar(currentSidebar(), railByWidth()) === "expanded"
          ? "rail"
          : "expanded",
      );
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);
  const name = t(expanded ? "sidebar.collapse" : "sidebar.expand");
  // Both names and icons, each hidden by the rule that picks the rail (globals.css) and, once known, by the state
  // here: the toggle is named for the layout the reader sees before hydration too (WEB-U-007).
  const forState = (
    layout: "expanded" | "rail",
    glyph: typeof PanelLeftClose,
    words: string,
  ) => (
    <span
      className="sidebar-toggle-state"
      data-sidebar-for={layout}
      hidden={known && (layout === "expanded") !== expanded}
    >
      <Icon glyph={glyph} />
      <span className="sidebar-toggle-name">{words}</span>
    </span>
  );
  return (
    <Button
      variant="ghost"
      className="icon-button sidebar-toggle"
      title={known ? `${name} ([)` : undefined}
      aria-keyshortcuts="["
      onClick={() => setSidebar(expanded ? "rail" : "expanded")}
    >
      {forState("expanded", PanelLeftClose, t("sidebar.collapse"))}
      {forState("rail", PanelLeftOpen, t("sidebar.expand"))}
    </Button>
  );
}
