"use client";

import {
  Children,
  isValidElement,
  useId,
  useState,
  type OptionHTMLAttributes,
  type ReactNode,
} from "react";

import { useT } from "../i18n/I18nProvider";

import { MenuButton, type MenuChoice } from "./MenuButton";

type Option = OptionHTMLAttributes<HTMLOptionElement> & {
  children?: ReactNode;
};

/** The words of an option, however its label is wrapped. */
function words(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(words).join("");
  if (isValidElement<{ children?: ReactNode }>(node))
    return words(node.props.children);
  return "";
}

/**
 * The standard picker in a form (WEB-U-134): its label above a field-looking button that opens the standard menu,
 * never a native select. Its choices are written as `<option>`s; a disabled option with an empty value is what it says
 * while nothing is chosen, any other disabled one is listed as unavailable. Its error is announced and marks it for
 * FormRefusal. Without `value` it keeps its own choice, from `defaultValue`, and a form reads it by `name`.
 */
export function Select({
  label,
  id: buttonId,
  value: controlled,
  defaultValue,
  onChange,
  name,
  error,
  disabled = false,
  "aria-describedby": describedBy,
  children,
}: {
  label: string;
  /** Its button's id, for what focuses it. */
  id?: string;
  value?: string;
  defaultValue?: string;
  onChange?: (value: string) => void;
  /** The form field its choice is sent as. */
  name?: string;
  error?: string | undefined;
  disabled?: boolean;
  "aria-describedby"?: string | undefined;
  children: ReactNode;
}) {
  const t = useT();
  const id = useId();
  const all = Children.toArray(children).flatMap((child) =>
    isValidElement<Option>(child) && child.type === "option"
      ? [child.props]
      : [],
  );
  const valueOf = (option: Option) =>
    option.value === undefined ? words(option.children) : String(option.value);
  const placeholder = all.find(
    (option) => option.disabled && valueOf(option) === "",
  );
  const options: MenuChoice[] = all
    .filter((option) => option !== placeholder)
    .map((option) => ({
      id: valueOf(option),
      label: words(option.children),
      ...(option.lang ? { lang: option.lang } : {}),
      ...(option.disabled ? { disabled: t("menu.unavailable") } : {}),
    }));
  const [own, setOwn] = useState(
    () => defaultValue ?? (options[0] ? options[0].id : ""),
  );
  const value = controlled ?? own;
  return (
    <div className="field">
      <span className="type-small" aria-hidden="true">
        {label}
      </span>
      <MenuButton
        label={label}
        options={options}
        value={value}
        onChange={(next) => {
          setOwn(next);
          onChange?.(next);
        }}
        disabled={disabled}
        field
        buttonId={buttonId}
        invalid={Boolean(error)}
        describedBy={
          [describedBy, error ? `${id}-error` : undefined]
            .filter(Boolean)
            .join(" ") || undefined
        }
        {...(placeholder ? { placeholder: words(placeholder.children) } : {})}
      />
      {name !== undefined && <input type="hidden" name={name} value={value} />}
      {error && (
        <p id={`${id}-error`} role="alert" className="field-error type-small">
          {error}
        </p>
      )}
    </div>
  );
}
