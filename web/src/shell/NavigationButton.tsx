"use client";

import { Menu } from "lucide-react";

import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";

import { useShell } from "./shell-context";

/**
 * Open navigation: below 768 px the sidebar is a drawer, and every page in the shell starts its bar with this button, or
 * a phone has no way to New task, the task list or the Account menu (WEB-U-362, WEB-E2E-086). Wider, it isn't shown
 * (`.navigation-button`). Nothing outside the shell (tests, the sign-in pages).
 */
export function NavigationButton() {
  const t = useT();
  const shell = useShell();
  if (!shell) return null;
  return (
    <Button
      variant="ghost"
      className="icon-button navigation-button"
      aria-label={t("shell.openNavigation")}
      onClick={shell.openNavigation}
    >
      <Icon glyph={Menu} />
    </Button>
  );
}
