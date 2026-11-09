"use client";

import { useQueryClient } from "@tanstack/react-query";
import {
  BarChart3,
  Check,
  ChevronDown,
  ChevronRight,
  CircleHelp,
  ExternalLink,
  Info,
  Languages,
  LogOut,
  type LucideIcon,
  MessageSquareText,
  Settings,
  Terminal,
} from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { type ReactNode, useState } from "react";

import { api } from "../api/client";
import { useRequest } from "../api/use-request";
import { useLanguage, useT } from "../i18n/I18nProvider";
import {
  FEEDBACK_URL,
  HELP_URL,
  LICENCE_URL,
  RELEASES_URL,
  REPOSITORY_URL,
} from "../lib/app";
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
  const t = useT();
  const { language, languages, setLanguage } = useLanguage();
  const [choosingLanguage, setChoosingLanguage] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const admin = session.data?.role === "admin";
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
          {/* WEB-U-463: who is signed in, then what people reach for; theme lives in Settings › Account. */}
          <p className="menu-heading type-small muted">
            {`${session.data.email} · ${t(admin ? "account.roleAdmin" : "account.roleMember")}`}
          </p>
          <MenuLink href="/settings" icon={Settings} onNavigate={onNavigate}>
            {t("account.settings")}
          </MenuLink>
          {admin && (
            <MenuLink
              href="/settings/usage"
              icon={BarChart3}
              onNavigate={onNavigate}
            >
              {t("account.usage")}
            </MenuLink>
          )}
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            tabIndex={-1}
            aria-expanded={choosingLanguage}
            onClick={() => setChoosingLanguage(!choosingLanguage)}
          >
            <Icon glyph={Languages} className="menu-icon" />
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
          <ExternalItem
            href={HELP_URL}
            icon={CircleHelp}
            newTab={t("account.newTab")}
          >
            {t("account.help")}
          </ExternalItem>
          <ExternalItem
            href={FEEDBACK_URL}
            icon={MessageSquareText}
            newTab={t("account.newTab")}
          >
            {t("account.feedback")}
          </ExternalItem>
          <hr className="menu-separator" />
          <MenuLink
            href="/settings/tokens"
            icon={Terminal}
            onNavigate={onNavigate}
          >
            {t("account.apiTokens")}
          </MenuLink>
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            tabIndex={-1}
            aria-expanded={aboutOpen}
            onClick={() => setAboutOpen(!aboutOpen)}
          >
            <Icon glyph={Info} className="menu-icon" />
            <span>{t("account.about")}</span>
            <Icon glyph={aboutOpen ? ChevronDown : ChevronRight} />
          </button>
          {aboutOpen && (
            <div role="group" aria-label={t("account.about")}>
              <ExternalItem
                href={RELEASES_URL}
                newTab={t("account.newTab")}
                sub
              >
                {t("account.whatsNew")}
              </ExternalItem>
              <ExternalItem
                href={REPOSITORY_URL}
                newTab={t("account.newTab")}
                sub
              >
                {t("account.source")}
              </ExternalItem>
              <ExternalItem href={LICENCE_URL} newTab={t("account.newTab")} sub>
                {t("account.licence")}
              </ExternalItem>
            </div>
          )}
          <hr className="menu-separator" />
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
            <Icon glyph={LogOut} className="menu-icon" />
            <span>
              {t(request.pending ? "account.signingOut" : "account.signOut")}
            </span>
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

/** An item that opens a page of the app: its icon, its words (WEB-U-463). */
function MenuLink({
  href,
  icon,
  onNavigate,
  children,
}: {
  href: string;
  icon: LucideIcon;
  onNavigate: () => void;
  children: ReactNode;
}) {
  return (
    <Link
      role="menuitem"
      tabIndex={-1}
      className="menu-item"
      href={href}
      onClick={onNavigate}
    >
      <Icon glyph={icon} className="menu-icon" />
      <span>{children}</span>
    </Link>
  );
}

/** An item that opens the project's site in a new tab, and says so to screen readers (WEB-U-463). */
function ExternalItem({
  href,
  icon,
  newTab,
  sub = false,
  children,
}: {
  href: string;
  icon?: LucideIcon;
  newTab: string;
  sub?: boolean;
  children: ReactNode;
}) {
  return (
    <a
      role="menuitem"
      tabIndex={-1}
      className={sub ? "menu-item menu-subitem" : "menu-item"}
      href={href}
      target="_blank"
      rel="noopener noreferrer"
    >
      {icon && <Icon glyph={icon} className="menu-icon" />}
      <span>
        {children} <span className="visually-hidden">{newTab}</span>
      </span>
      <Icon glyph={ExternalLink} size={16} />
    </a>
  );
}
