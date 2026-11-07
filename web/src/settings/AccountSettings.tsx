"use client";

import { useLanguage, useT } from "../i18n/I18nProvider";
import type { User } from "../protocol/generated/api";
import { ThemeSelect } from "../shell/ThemeSelect";
import { Select } from "../ui/Select";

/**
 * Your account › Account (WEB-U-078): who you are, and the theme and language, which the account menu also sets. Each
 * language is named in its own script, with a note in the reader's language when it is only partly translated.
 */
export function AccountSettings({ me }: { me: User }) {
  const t = useT();
  const { language, languages, setLanguage } = useLanguage();
  return (
    <div className="stack account-settings">
      <p className="type-small muted">{t("accountPage.intro")}</p>
      <dl className="account-facts">
        <dt className="type-label muted">{t("accountPage.email")}</dt>
        <dd>{me.email}</dd>
        <dt className="type-label muted">{t("users.role")}</dt>
        <dd>{t("users.roleName", { role: me.role })}</dd>
      </dl>
      <ThemeSelect />
      <Select
        label={t("account.language")}
        value={language}
        onChange={(choice) => setLanguage(choice)}
      >
        {languages.map((entry) => (
          <option key={entry.code} value={entry.code} lang={entry.code}>
            {entry.complete
              ? entry.name
              : `${entry.name} ${t("account.languagePartial")}`}
          </option>
        ))}
      </Select>
    </div>
  );
}
