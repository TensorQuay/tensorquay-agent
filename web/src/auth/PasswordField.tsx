"use client";

import "./catalogue";
import { Check, Circle, Eye, EyeOff } from "lucide-react";
import { useId, useState } from "react";

import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";
import { Input } from "../ui/Input";

/** Shows the public password rules as the administrator types; the gateway still enforces its policy. */
export function PasswordField({
  value,
  onChange,
  email,
  setup,
  disabled,
  error,
}: {
  value: string;
  onChange: (value: string) => void;
  email: string;
  setup: boolean;
  disabled: boolean;
  /** The form's own words for an empty or short password (WEB-U-355); the browser's bubble is off (`noValidate`). */
  error?: string | undefined;
}) {
  const [visible, setVisible] = useState(false);
  const rulesId = useId();
  const t = useT();
  const rules = [
    { label: t("password.length"), met: Array.from(value).length >= 12 },
    {
      label: t("password.notEmail"),
      met:
        value.length > 0 &&
        (!email.trim() ||
          !value.toLowerCase().includes(email.trim().toLowerCase())),
    },
  ];
  return (
    <div className="password-field stack">
      <div className="password-input">
        <Input
          label={t("password.label")}
          name="password"
          type={visible ? "text" : "password"}
          autoComplete={setup ? "new-password" : "current-password"}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          required
          minLength={setup ? 12 : 1}
          maxLength={1024}
          disabled={disabled}
          aria-describedby={setup ? rulesId : undefined}
          {...(error ? { error } : {})}
          adornment={
            <Button
              variant="ghost"
              className="password-toggle icon-button"
              aria-label={t(visible ? "password.hide" : "password.show")}
              title={t(visible ? "password.hide" : "password.show")}
              onClick={() => setVisible(!visible)}
              disabled={disabled}
            >
              <Icon glyph={visible ? EyeOff : Eye} />
            </Button>
          }
        />
      </div>
      {setup && (
        <ul className="password-rules type-small muted" id={rulesId}>
          {rules.map(({ label, met }) => (
            <li
              key={label}
              aria-label={t("password.ruleState", {
                rule: label,
                met: met ? "yes" : "no",
              })}
              data-met={met}
            >
              <Icon glyph={met ? Check : Circle} size={16} />
              {label}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
