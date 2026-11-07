"use client";

import { useId, type ReactNode } from "react";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Input } from "../ui/Input";

import { type Draft, type Problems } from "./route-copy";
import { PageSection } from "./SettingsPage";

/** The draft and its setters, as each part of the route form takes them (the form's pieces share it). */
export type Section = {
  draft: Draft;
  set: (change: Partial<Draft>) => void;
  problems: Problems;
  /** A protocol change, which resets an instructions role the new protocol can't send (WEB-U-057). */
  switchProtocol: (protocol: Draft["protocol"]) => void;
  roleReset: boolean;
};

/** A setting a failed route check suggests (WEB-U-058): applied to the draft, never saved without the person. */
export type Preset = {
  field:
    | "strict"
    | "schema_keywords"
    | "max_output"
    | "stream_usage"
    | "context_window"
    | "base_url";
  value: string;
};

/** A text field bound to one draft key, with its problem from the client or the gateway. */
export function Text({
  section,
  name,
  labelKey,
  ...props
}: {
  section: Section;
  name: keyof Draft;
  labelKey: MessageKey;
  type?: string;
  list?: string;
  inputMode?: "numeric";
  placeholder?: string;
  /** The field's id, for a suggestion that focuses it (WEB-U-133). */
  id?: string;
  /** A hint under the field, read with it. */
  hintKey?: MessageKey;
}) {
  const t = useT();
  const problem = section.problems[name];
  const hintId = useId();
  const { hintKey, ...rest } = props;
  return (
    <>
      <Input
        label={t(labelKey)}
        value={section.draft[name] as string}
        onChange={(event) => section.set({ [name]: event.target.value })}
        autoComplete="off"
        spellCheck={false}
        {...rest}
        {...(hintKey ? { "aria-describedby": hintId } : {})}
        {...(problem ? { error: t(problem) } : {})}
      />
      {hintKey && (
        <p id={hintId} className="type-small muted">
          {t(hintKey)}
        </p>
      )}
    </>
  );
}

/**
 * One section of the route's page (WEB-U-115): its heading, then its fields as one block in the page's fields column
 * from 1024 px (the page frame's PageSection, whose parts here are richer than a label and a field).
 */
export function Group({
  legend,
  children,
}: {
  legend: MessageKey;
  children: ReactNode;
}) {
  const t = useT();
  return (
    <PageSection title={t(legend)}>
      <div className="page-section-body stack">{children}</div>
    </PageSection>
  );
}
