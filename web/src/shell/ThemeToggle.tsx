"use client";

import { Monitor, Moon, Sun } from "lucide-react";
import { useSyncExternalStore } from "react";

import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";

import { currentTheme, setTheme, subscribeTheme, type Theme } from "./theme";
import "../i18n/deferred-catalogue";

const themes: readonly Theme[] = ["system", "light", "dark"];

/** Cycles the theme by click or arrow keys with an accessible current-choice label. */
export function ThemeToggle() {
  const theme = useSyncExternalStore(
    subscribeTheme,
    currentTheme,
    () => "system" as const,
  );
  const t = useT();
  const cycle = (direction: number) =>
    setTheme(
      themes[
        (themes.indexOf(theme) + direction + themes.length) % themes.length
      ] ?? "system",
    );
  return (
    <Button
      variant="ghost"
      className="icon-button"
      aria-label={t("theme.current", { theme })}
      onClick={() => cycle(1)}
      onKeyDown={(event) => {
        if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
          event.preventDefault();
          cycle(event.key === "ArrowRight" ? 1 : -1);
        }
      }}
    >
      <Icon
        glyph={theme === "light" ? Sun : theme === "dark" ? Moon : Monitor}
      />
    </Button>
  );
}
