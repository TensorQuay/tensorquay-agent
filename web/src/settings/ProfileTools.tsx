"use client";

import { useId } from "react";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { CheckRow } from "../ui/CheckRow";

import {
  TOOL_GROUPS,
  TOOLS,
  type EngineChoice,
  type ProfileTool,
} from "./profiles";

/** The choices a profile can narrow to: what this person's tasks can use today. */
export type ProfileChoices = {
  /** The model routes by the name people know, sent by their route name. */
  routes: { name: string; label: string }[];
  templates: { name: string; label: string }[];
  connectors: { name: string; label: string }[];
  /**
   * The administrator's tool servers, offered to shared profiles only (WEB-U-059): absent for members, "failed" when
   * they could not be loaded. `templates` are the environments' display names; none means every environment.
   */
  servers?:
    { name: string; templates: string[]; byDefault: boolean }[] | "failed";
  /** The decision engines, for administrators only: absent for members, "failed" when not loaded. */
  engines?: EngineChoice[] | "failed";
};

const GROUP_LABELS: Readonly<
  Record<(typeof TOOL_GROUPS)[number][0], MessageKey>
> = {
  files: "profiles.toolGroupFiles",
  finding: "profiles.toolGroupFinding",
  commands: "profiles.toolGroupCommands",
  notes: "profiles.toolGroupNotes",
  web: "profiles.toolGroupWeb",
  skills: "profiles.toolGroupSkills",
  channels: "profiles.toolGroupChannels",
};
const TOOL_LABELS: Readonly<Record<ProfileTool, [MessageKey, MessageKey]>> = {
  read_file: ["profiles.toolReadFile", "profiles.toolReadFileHint"],
  write_file: ["profiles.toolWriteFile", "profiles.toolWriteFileHint"],
  edit_file: ["profiles.toolEditFile", "profiles.toolEditFileHint"],
  glob: ["profiles.toolGlob", "profiles.toolGlobHint"],
  grep: ["profiles.toolGrep", "profiles.toolGrepHint"],
  read_notes: ["profiles.toolReadNotes", "profiles.toolReadNotesHint"],
  write_notes: ["profiles.toolWriteNotes", "profiles.toolWriteNotesHint"],
  shell: ["profiles.toolShell", "profiles.toolShellHint"],
  skill: ["profiles.toolSkill", "profiles.toolSkillHint"],
  fetch_url: ["profiles.toolFetchUrl", "profiles.toolFetchUrlHint"],
  computer: ["profiles.toolComputer", "profiles.toolComputerHint"],
  channel_post: ["profiles.toolChannelPost", "profiles.toolChannelPostHint"],
  channel_read: ["profiles.toolChannelRead", "profiles.toolChannelReadHint"],
  channel_wait: ["profiles.toolChannelWait", "profiles.toolChannelWaitHint"],
  task_claim: ["profiles.toolTaskClaim", "profiles.toolTaskClaimHint"],
};

/**
 * A profile's tools, always listed (WEB-U-105): every built-in tool in its group card, ticked unless the profile leaves
 * it out, each with one plain line on what it lets the agent do, and All above the cards to tick or clear them at once
 * (mixed while some are ticked).
 */
export function ProfileTools({
  value,
  onChange,
  disabled,
  error,
}: {
  value: readonly ProfileTool[];
  onChange: (tools: ProfileTool[]) => void;
  disabled: boolean;
  error: string | undefined;
}) {
  const t = useT();
  const id = useId();
  const every = TOOLS.every((tool) => value.includes(tool));
  const some = !every && value.length > 0;
  return (
    // The whole content width (WEB-U-131): All above a grid of group cards, never one narrow column beside the labels'.
    <div className="page-section-list stack profile-tools">
      <CheckRow
        className="type-body-strong"
        checked={every}
        mixed={some}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked ? [...TOOLS] : [])}
        label={t("profiles.allTools")}
      />
      <div className="choice-grid">
        {TOOL_GROUPS.map(([group, tools]) => (
          <fieldset key={group} className="tool-group">
            <legend className="type-small muted">
              {t(GROUP_LABELS[group])}
            </legend>
            {tools.map((tool) => {
              const [label, hint] = TOOL_LABELS[tool];
              return (
                <div key={tool} className="tool-row">
                  <CheckRow
                    className="type-small"
                    checked={value.includes(tool)}
                    disabled={disabled}
                    aria-describedby={`${id}-${tool}`}
                    onChange={(event) =>
                      onChange(
                        event.target.checked
                          ? TOOLS.filter(
                              (each) => each === tool || value.includes(each),
                            )
                          : value.filter((each) => each !== tool),
                      )
                    }
                    label={t(label)}
                  />
                  <p
                    id={`${id}-${tool}`}
                    className="type-small muted tool-hint"
                  >
                    {t(hint)}
                  </p>
                </div>
              );
            })}
          </fieldset>
        ))}
      </div>
      {error && <p className="field-error">{error}</p>}
    </div>
  );
}
