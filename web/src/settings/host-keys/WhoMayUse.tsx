"use client";

import { X } from "lucide-react";
import { useId } from "react";

import { useLanguage, useT } from "../../i18n/I18nProvider";
import { CheckRow } from "../../ui/CheckRow";
import { IconAction } from "../../ui/IconAction";
import { MenuButton } from "../../ui/MenuButton";
import { PageSection } from "../SettingsPage";

/**
 * Who may use a shared key (ADR-0037 §1, administrators only): everyone in the installation, or chosen people, added
 * with the standard picker (search past 7) and shown as chips, each with its ×.
 */
export function WhoMayUse({
  people,
  users,
  error,
  onChange,
}: {
  /** The installation's people, by id. */
  people: Record<string, string>;
  users: "all" | string[];
  error: string | undefined;
  onChange: (users: "all" | string[]) => void;
}) {
  const t = useT();
  const { language } = useLanguage();
  const name = useId();
  const errorId = useId();
  const chosen = users === "all" ? [] : users;
  const choices = Object.entries(people)
    .filter(([id]) => !chosen.includes(id))
    .sort(([, a], [, b]) => a.localeCompare(b, language))
    .map(([id, email]) => ({ id, label: email }));
  return (
    <PageSection title={t("hostKeys.whoSection")}>
      <fieldset
        className="route-group radio-options"
        role="radiogroup"
        aria-label={t("hostKeys.whoSection")}
      >
        <CheckRow
          type="radio"
          className="type-small"
          name={name}
          checked={users === "all"}
          onChange={() => onChange("all")}
          label={t("hostKeys.everyone")}
        />
        <CheckRow
          type="radio"
          className="type-small"
          name={name}
          checked={users !== "all"}
          onChange={() => onChange(chosen)}
          label={t("hostKeys.chosenPeople")}
        />
      </fieldset>
      {users !== "all" && (
        <div className="stack">
          <MenuButton
            label={t("hostKeys.addPerson")}
            placeholder={t("hostKeys.choosePerson")}
            value=""
            options={choices}
            disabled={choices.length === 0}
            // The picker takes the error and the focus: a fieldset can't be focused.
            invalid={Boolean(error)}
            describedBy={error ? errorId : undefined}
            onChange={(id) => onChange([...chosen, id])}
          />
          {chosen.length > 0 && (
            <ul
              className="composer-chips"
              aria-label={t("hostKeys.chosenList")}
            >
              {chosen.map((id) => {
                // Someone chosen who has since left the installation: never their raw id.
                const email = people[id] ?? t("usage.removedAccount");
                return (
                  <li key={id} className="composer-chip">
                    <span className="type-small">{email}</span>
                    <IconAction
                      glyph={X}
                      name={t("hostKeys.removePerson", { person: email })}
                      tip={t("hostKeys.removePerson", { person: email })}
                      onClick={() =>
                        onChange(chosen.filter((other) => other !== id))
                      }
                    />
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}
      {error && (
        <p id={errorId} role="alert" className="field-error type-small">
          {error}
        </p>
      )}
    </PageSection>
  );
}
