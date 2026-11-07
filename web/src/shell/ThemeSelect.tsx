"use client";

import { useSyncExternalStore } from "react";

import { useT } from "../i18n/I18nProvider";
import { Select } from "../ui/Select";

import { currentTheme, setTheme, subscribeTheme } from "./theme";
import "../i18n/deferred-catalogue";

/** Uses the shared persisted theme preference from the account menu. */
export function ThemeSelect() {
  const theme = useSyncExternalStore(
    subscribeTheme,
    currentTheme,
    () => "system" as const,
  );
  const t = useT();
  return (
    <Select
      label={t("theme.label")}
      value={theme}
      onChange={(choice) => {
        const value = choice;
        if (value === "system" || value === "light" || value === "dark")
          setTheme(value);
      }}
    >
      <option value="system">{t("theme.system")}</option>
      <option value="light">{t("theme.light")}</option>
      <option value="dark">{t("theme.dark")}</option>
    </Select>
  );
}
