"use client";

import { useQueryClient } from "@tanstack/react-query";
import { Check, ChevronDown, ChevronRight } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState, useSyncExternalStore } from "react";

import { api } from "../api/client";
import { useRequest } from "../api/use-request";
import { useLanguage, useT } from "../i18n/I18nProvider";
import { currentTheme, setTheme, subscribeTheme } from "../shell/theme";
import { ErrorMessage } from "../ui/ErrorMessage";
import { Icon } from "../ui/Icon";
import { Menu } from "../ui/Menu";

import { useSession } from "./use-session";
import "../i18n/deferred-catalogue";

/**
 * Account preferences load on demand; failed sign-out stays visible and can be retried. `onNavigate` (default
 * `onClose`) is called for an item that leaves the page: Settings, a sign-out that succeeded.
 */
export function AccountActions({
  onClose,
  onNavigate = onClose,
}: {
  onClose: () => void;
  onNavigate?: () => void;
}) {
  const session = useSession(false);
  const request = useRequest();
  const query = useQueryClient();
  const router = useRouter();
  const theme = useSyncExternalStore(
    subscribeTheme,
    currentTheme,
    () => "system" as const,
  );
  const t = useT();
  const { language, languages, setLanguage } = useLanguage();
  const [choosingLanguage, setChoosingLanguage] = useState(false);
  const languageName =
    languages.find((entry) => entry.code === language)?.name ?? language;
  async function logout() {
    const result = await request.run((signal) => api.logout(signal));
    if (result.ok) {
      query.clear();
      onNavigate();
      router.replace("/login");
    }
  }
  return (
    <Menu label={t("account.label")}>
      {session.data ? (
        <>
          <p className="menu-heading type-small muted">{session.data.email}</p>
          <Link
            role="menuitem"
            tabIndex={-1}
            className="menu-item"
            href="/settings"
            onClick={onNavigate}
          >
            {t("account.settings")}
          </Link>
          <div role="group" aria-label={t("theme.label")}>
            <p className="menu-heading type-small muted">{t("theme.label")}</p>
            {(["system", "light", "dark"] as const).map((value) => (
              <button
                type="button"
                key={value}
                role="menuitemradio"
                className="menu-item"
                tabIndex={-1}
                aria-checked={theme === value}
                onClick={() => setTheme(value)}
              >
                <span>{t(`theme.${value}`)}</span>
                <span className="menu-check">
                  {theme === value && <Icon glyph={Check} size={16} />}
                </span>
              </button>
            ))}
          </div>
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            tabIndex={-1}
            aria-expanded={choosingLanguage}
            onClick={() => setChoosingLanguage(!choosingLanguage)}
          >
            <span>
              {t("account.languageCurrent", { language: languageName })}
            </span>
            <Icon glyph={choosingLanguage ? ChevronDown : ChevronRight} />
          </button>
          {choosingLanguage && (
            <div role="group" aria-label={t("account.language")}>
              {languages.map((entry) => (
                <button
                  type="button"
                  key={entry.code}
                  role="menuitemradio"
                  className="menu-item menu-subitem"
                  tabIndex={-1}
                  lang={entry.code}
                  aria-checked={language === entry.code}
                  onClick={() => setLanguage(entry.code)}
                >
                  <span>
                    {entry.name}
                    {!entry.complete && (
                      // The name is in its own language; the note is in the reader's (WEB-U-032).
                      <>
                        {" "}
                        <span lang={language} className="muted">
                          {t("account.languagePartial")}
                        </span>
                      </>
                    )}
                  </span>
                  <span className="menu-check">
                    {language === entry.code && (
                      <Icon glyph={Check} size={16} />
                    )}
                  </span>
                </button>
              ))}
            </div>
          )}
          <ErrorMessage error={request.error} />
          <button
            type="button"
            role="menuitem"
            className="menu-item account-signout"
            tabIndex={-1}
            disabled={request.pending}
            onClick={() => {
              void logout();
            }}
          >
            {t(request.pending ? "account.signingOut" : "account.signOut")}
          </button>
        </>
      ) : (
        <Link
          role="menuitem"
          tabIndex={-1}
          className="menu-item"
          href="/login"
          onClick={onClose}
        >
          {t("account.signIn")}
        </Link>
      )}
    </Menu>
  );
}
