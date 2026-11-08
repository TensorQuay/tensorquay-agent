import { Box } from "lucide-react";

import { useLanguage, useT } from "../i18n/I18nProvider";
import { formatMib } from "../i18n/message";
import { MenuButton } from "../ui/MenuButton";

import { capabilityNames, type Capability } from "./capabilities";

/** A presentation choice; the adapter supplies its identity, display name, declared capabilities and purpose. */
export type EnvironmentChoice = {
  id: string;
  label: string;
  capabilities: readonly Capability[];
  /** The administrator's one line on what it is for; null when there is none. */
  description: string | null;
  /** Its effective CPUs and memory, shown at the end of its line (the sizes are each environment's). */
  size?: { cpus: number; memoryMib: number } | undefined;
};

/**
 * Reuses the common menu without issuing requests; the caller owns confirmation and persistence. Each choice reads by
 * what it is for when the administrator said so, its tools then on hover; otherwise its tools in words, never icons.
 */
export function EnvironmentPicker({
  choices,
  value,
  onChange,
  disabled = false,
  disabledReason,
  placement,
}: {
  choices: readonly EnvironmentChoice[];
  value: string;
  onChange: (identity: string) => void;
  disabled?: boolean;
  disabledReason?: string | undefined;
  /** Where its choices open (the task header's open below, aligned to their end). */
  placement?: "below-start" | "below-end" | "above-start" | undefined;
}) {
  const t = useT();
  const { language } = useLanguage();
  // The size as every size is written, and kept on one line (WEB-U-185).
  const size = (choice: EnvironmentChoice) =>
    choice.size && (
      <>
        {" · "}
        <span className="nowrap">
          {`${t("picker.cpus", { cpus: choice.size.cpus })} · ${formatMib(
            choice.size.memoryMib,
            language,
          )}`}
        </span>
      </>
    );
  // Undescribed, the line lists the tools, and a desktop is marked after them as a described one is, never one more
  // of their commas (a pass over the pages before 17.0).
  const tools = (values: readonly Capability[]) => {
    const others = values.filter((value) => value !== "desktop");
    if (others.length === values.length) return capabilityNames(values, t);
    return others.length > 0
      ? `${capabilityNames(others, t)} · ${t("capability.desktop")}`
      : t("capability.desktop");
  };
  return (
    <MenuButton
      label={t("picker.environment")}
      value={value}
      onChange={onChange}
      disabled={disabled}
      disabledReason={disabledReason}
      {...(placement ? { placement } : {})}
      icon={Box}
      options={choices.map((choice) => ({
        id: choice.id,
        label: choice.label,
        detail: choice.description ? (
          <span
            className="type-small muted"
            title={capabilityNames(choice.capabilities, t)}
          >
            {choice.description}
            {/* Described, its tools are only the tooltip; a desktop says so where it is read (a UX review before 17.0),
                as its size does. */}
            {choice.capabilities.includes("desktop") &&
              ` · ${t("capability.desktop")}`}
            {size(choice)}
          </span>
        ) : (
          <span className="type-small muted">
            {tools(choice.capabilities)}
            {size(choice)}
          </span>
        ),
      }))}
    />
  );
}
