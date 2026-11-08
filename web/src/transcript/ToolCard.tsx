"use client";

import { ChevronDown, ChevronRight } from "lucide-react";
import { useId, useState, type ReactNode } from "react";

import { useT } from "../i18n/I18nProvider";
import { ScreenPictures } from "../screen/Pictures";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";
import { StatusPill, type Status } from "../ui/StatusPill";

import { connectorName } from "./connector-call";
import { unmarked } from "./marker";
import { argumentLine, durationLabel } from "./presentation";
import { pushFailure } from "./push-failure";
import { teamFailure } from "./team-failure";
import { TEAM_READS, teamReadCount } from "./team-read";
import { argument, needsGitCredential } from "./tool-results";
import { namedRef } from "./tool-summary";
import { foldTraceback } from "./traceback";
import type { ToolView } from "./types";
import "../i18n/deferred-catalogue";

const previewLines = 20;
const statuses: Record<ToolView["status"], Status> = {
  // Called, not yet started (ADR-0035: nothing waits for approval).
  prepared: "starting",
  running: "running",
  ok: "completed",
  error: "failed",
  cancelled: "cancelled",
  interrupted: "failed",
};

/** Presents a call's recorded arguments, output and outcome without executing or interpreting them. */
export function ToolCard({
  tool,
  initiallyOpen = false,
  embedded = false,
}: {
  tool: ToolView;
  initiallyOpen?: boolean;
  /**
   * Opened from its activity line, which already names the call and its outcome: no heading of its
   * own, no internal tool name and no status badge; the command itself stays behind Details.
   */
  embedded?: boolean;
}) {
  const t = useT();
  const id = useId();
  const [choice, setChoice] = useState<{
    status: ToolView["status"];
    open: boolean;
  } | null>(null);
  const [showAll, setShowAll] = useState(false);
  const [details, setDetails] = useState(false);
  // A call with no arguments has no Details to offer, never a bare label.
  const hasArguments =
    tool.arguments != null &&
    !(
      typeof tool.arguments === "object" &&
      Object.keys(tool.arguments).length === 0
    );
  const neverRan = !tool.ran && tool.result !== null;
  // A failure its step already says in words (every failed connector call, a worded push) keeps its raw result folded
  // in Result: the page never shows the runtime's line ("key_use_unrecorded: …", WEB-E2E-029).
  const worded =
    tool.status === "error" &&
    (connectorName(tool.name) !== null ||
      pushFailure(tool) !== null ||
      teamFailure(tool) !== null ||
      needsGitCredential(tool));
  const expanded =
    choice?.status === tool.status
      ? choice.open
      : initiallyOpen ||
        neverRan ||
        (tool.status !== "ok" && tool.status !== "prepared");
  // A team read's answer is the model's marked blocks ("<signed-in-person …>"): the card says how many messages it read,
  // each of which the team's log words, and never shows the blocks, open or folded (WEB-U-327).
  const teamRead = TEAM_READS.has(tool.name) && tool.status === "ok";
  // A worded failure's raw text stays in the closed Result, whether it came as the result or streamed as output first
  // (the runtime sends both, WEB-U-186); anything else shows its output, and its result when it has no output.
  // A marked block's tag lines are the model's (tqa-tools untrusted.rs): the card shows what's inside (WEB-U-363).
  const shownOutput =
    worded || teamRead ? "" : namedRef(unmarked(tool.output), tool, t);
  const desktop = tool.name === "computer";
  const given = teamRead
    ? null
    : worded
      ? (tool.result ?? (tool.output || null))
      : tool.output
        ? null
        : tool.result;
  // A blank result has nothing to show: no Result heading over nothing (a pass over the pages before 17.0).
  const shown = given === null ? null : namedRef(unmarked(given), tool, t);
  const folded = shown?.trim() ? shown : null;
  // A failed step's Python traceback reads as its last line until Show more (WEB-U-400).
  const trace =
    tool.status === "error"
      ? foldTraceback(shownOutput)
      : { text: shownOutput, folded: false };
  const lines = trace.text.split("\n");
  // A running command's output grows line by line: its latest lines show as they come (a closing newline is no line of
  // its own); once ended, its first (17.3).
  const output = showAll
    ? shownOutput
    : (tool.status === "running"
        ? trace.text.replace(/\n$/, "").split("\n").slice(-previewLines)
        : lines.slice(0, previewLines)
      ).join("\n");
  return (
    <article
      className={embedded ? "tool-card tool-card-embedded" : "tool-card"}
      aria-label={t("toolcard.label", { name: tool.name })}
    >
      {!embedded && (
        <button
          className="tool-heading"
          type="button"
          aria-expanded={expanded}
          aria-controls={id}
          title={
            tool.ran && tool.durationMs !== null
              ? durationLabel(tool.durationMs)
              : undefined
          }
          onClick={() => setChoice({ status: tool.status, open: !expanded })}
        >
          <Icon glyph={ChevronDown} className="disclosure-icon" />
          <span className="type-code tool-name">{tool.name}</span>
          <StatusPill status={statuses[tool.status]} />
          {tool.ran && tool.durationMs !== null && tool.durationMs > 1000 && (
            <span className="type-label muted">
              {durationLabel(tool.durationMs, t)}
            </span>
          )}
        </button>
      )}
      <div id={id} className="tool-body" hidden={!embedded && !expanded}>
        {/* A desktop step's pictures are what it did, so they come first; its arguments (an element's ref the model
            used, "e7") come last (a UX review before 17.0). */}
        {desktop && <ScreenPictures tool={tool} />}
        {!embedded && !desktop && (
          <ScrollBlock label={t("toolcard.arguments")}>
            {argumentLine(tool)}
          </ScrollBlock>
        )}
        {teamRead && (
          <p className="type-small">
            {t("toolcard.teamRead", {
              count: teamReadCount(tool.result ?? tool.output),
            })}
          </p>
        )}
        {shownOutput && (
          <>
            {/* The output has its own label, never under the arguments. */}
            <p className="type-label muted">{t("toolcard.outputHeading")}</p>
            <ScrollBlock label={t("toolcard.output")}>{output}</ScrollBlock>
            {(lines.length > previewLines || trace.folded) && (
              <Button variant="ghost" onClick={() => setShowAll(!showAll)}>
                {t(showAll ? "toolcard.showLess" : "toolcard.showMore")}
              </Button>
            )}
          </>
        )}
        {!embedded && desktop && (
          <>
            {/* Below the output, the arguments say what they are, as the output does. */}
            <p className="type-label muted">{t("toolcard.arguments")}</p>
            <ScrollBlock label={t("toolcard.arguments")}>
              {argumentLine(tool)}
            </ScrollBlock>
          </>
        )}
        {/* The owner's direction (c8): the line names the project; the card says what the changes were saved as. */}
        {tool.name === "git_push" &&
          tool.status === "ok" &&
          typeof argument(tool, "branch") === "string" && (
            <p className="type-label muted">
              {t("toolcard.savedAs", {
                name: String(argument(tool, "branch")),
              })}
            </p>
          )}
        {/* Only a command that didn't succeed says so, with its code; success says nothing. */}
        {tool.ran && tool.exitCode !== null && tool.exitCode !== 0 && (
          <p className="type-label muted">
            {t("toolcard.exitCode", { code: String(tool.exitCode) })}
          </p>
        )}
        {folded !== null &&
          (neverRan && !worded ? (
            <ScrollBlock label={t("toolcard.result")} result>
              {folded}
            </ScrollBlock>
          ) : (
            <details className="tool-fold">
              {/* A fold, so it shows the product's chevron, as Details does (WEB-U-330). */}
              <summary className="tool-disclosure type-label muted">
                <Icon glyph={ChevronRight} size={16} />
                {t("toolcard.result")}
              </summary>
              <ScrollBlock label={t("toolcard.result")} result>
                {folded}
              </ScrollBlock>
            </details>
          ))}
        {hasArguments && (
          <>
            {/* Quiet, like the Output label it lines up with (the review of 078). */}
            <button
              type="button"
              className="tool-disclosure type-label muted"
              aria-expanded={details}
              aria-controls={`${id}-arguments`}
              onClick={() => setDetails(!details)}
            >
              <Icon glyph={details ? ChevronDown : ChevronRight} size={16} />
              {t("toolcard.details")}
            </button>
            {details && (
              <div id={`${id}-arguments`}>
                <ScrollBlock label={t("toolcard.allArguments")}>
                  {JSON.stringify(tool.arguments, null, 2)}
                </ScrollBlock>
              </div>
            )}
          </>
        )}
      </div>
    </article>
  );
}

/**
 * A tool's code text: it can scroll sideways (a long command or one-line JSON), so a keyboard can reach it (UI-GUIDELINES
 * §9; axe scrollable-region-focusable). A labelled group, not a region: every open card has its Result, Output and
 * arguments, and named regions are page landmarks, so two open cards would repeat one (axe landmark-unique).
 */
function ScrollBlock({
  label,
  result = false,
  children,
}: {
  label: string;
  result?: boolean;
  children: ReactNode;
}) {
  return (
    <pre
      className={`code-block type-code${result ? " result-text" : ""}`}
      role="group"
      tabIndex={0}
      aria-label={label}
    >
      {children}
    </pre>
  );
}
