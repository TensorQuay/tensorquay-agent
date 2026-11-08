import { previewKind, shownInPanel } from "../files/file-kind";
import { workspacePath } from "../files/workspace-path";
import { english, type Translate } from "../i18n/translate";

import {
  failureCode,
  failureEngine,
  failureNext,
  failureProvider,
  failureReason,
} from "./failure-copy";
import { secondsLabel } from "./numbers";
import { duration } from "./steps";
import { argument, fileChange } from "./tool-results";
import { project } from "./tool-summary";
import type { RunRecord, StepView, ToolView } from "./types";

/** A test file by its usual names: a tests/ or spec/ folder, or a test_*, *_test, *.test.* or *.spec.* file. */
const TEST_FILE =
  /(^|\/)(tests?|spec)\/|(^|\/)test_[^/]*$|_test\.[^/]*$|\.(test|spec)\.[^/]*$/;
/** Documents and data are never tests, wherever they are (a general-purpose agent: a spec/ folder may hold prose). */
const NOT_CODE =
  /\.(md|markdown|txt|rst|adoc|docx?|odt|rtf|pdf|csv|tsv|xlsx?|json|ya?ml|html?|png|jpe?g|svg)$/i;

/** A next step under a finished turn: a message to send, or a file it made to open. */
export type FollowUp =
  | { kind: "send"; key: "followUp.runTests" }
  /** `chart`: the one image named like a chart, offered as "Open the chart". */
  | { kind: "open"; path: string; chart: boolean };

/** A path's file name, without its folder. */
export const fileName = (path: string) => path.slice(path.lastIndexOf("/") + 1);

/** How many files a take_files call placed: its fixed sentence, "Placed 2 files from report 4 in uploads/: …". */
function placedCount(result: string | null): number {
  const placed = /^Placed (\d+) files? from report /.exec(result ?? "");
  return placed ? Number(placed[1]) : 0;
}

/** The tools only a team's lead calls: a turn that called one led the team (ADR-0034). */
const LEADING: ReadonlySet<string> = new Set([
  "start_member",
  "stop_member",
  "read_helper",
  "take_files",
  "done",
  "assign",
  "ask_all",
  "decide",
  "ask_person",
]);

/** Summarizes recorded work once per turn, counting changed paths only once. */
export function turnSummary(
  run: RunRecord | undefined,
  steps: StepView[],
  tools: ToolView[],
  t: Translate = english,
  /**
   * The paths of the run's recorded changes, when there are any: they include files written by shell commands, and
   * leave out a file new under uploads/, which they take for the person's own upload (GW-I-048).
   */
  changedPaths?: readonly string[],
) {
  const created = new Set<string>();
  const updated = new Set<string>();
  for (const tool of tools) {
    const change = fileChange(tool);
    // The path the page opens: "/workspace/x" and "./x" are the workspace's x; a path outside it is never offered.
    const raw = argument(tool, "path");
    const path = typeof raw === "string" ? workspacePath(raw) : null;
    if (change && path !== null)
      (change.kind === "created" ? created : updated).add(path);
  }
  for (const path of created) updated.delete(path);
  const commands = tools.filter(
    (tool) => tool.name === "shell" && tool.ran,
  ).length;
  // Research reads pages; the heading counts them, as it counts commands.
  const pages = tools.filter(
    (tool) => tool.name === "fetch_url" && tool.status === "ok",
  ).length;
  // The owner's direction (c8): the projects it saved changes to, by site and path, never "pushed".
  const projects = new Set(
    tools
      .filter((tool) => tool.name === "git_push" && tool.status === "ok")
      .map((tool) => project(argument(tool, "remote")) ?? tool.id),
  ).size;
  const elapsed = run?.finishedAt
    ? duration(run.startedAt, run.finishedAt)
    : null;
  const recorded =
    elapsed ?? steps.reduce((total, step) => total + (step.durationMs ?? 0), 0);
  const work = [
    created.size ? t("summary.created", { count: created.size }) : "",
    updated.size ? t("summary.updated", { count: updated.size }) : "",
    pages ? t("summary.readPages", { count: pages }) : "",
    projects ? t("summary.savedProjects", { count: projects }) : "",
    commands ? t("summary.ran", { count: commands }) : "",
  ]
    .filter(Boolean)
    .join(t("summary.separator"));
  // A quick turn reads "under a second", never "0 s" (WEB-U-157).
  const took =
    recorded < 1000 ? t("summary.underSecond") : secondsLabel(recorded, t);
  const activityLabel = work
    ? t("summary.workedWith", { duration: took, work })
    : t("summary.worked", { duration: took });
  // The files it changed: the recorded changes and what its own write and edit calls made, each path once, so the
  // footer never says fewer than the summary's "created 1 file" (WEB-U-365: a file it wrote under uploads/).
  const written = [...created, ...updated];
  // Files a helper handed back that the turn took (take_files): as many as it placed, even when its recorded changes
  // leave them out (they land under uploads/, GW-I-048), never twice once the lead moved them (WEB-U-373).
  const taken = tools
    .filter((tool) => tool.name === "take_files" && tool.status === "ok")
    .reduce((count, tool) => count + placedCount(tool.result), 0);
  const files = Math.max(
    changedPaths ? new Set([...changedPaths, ...written]).size : written.length,
    taken,
  );
  const counts = [
    took,
    t("summary.steps", { count: steps.length }),
    t("summary.files", { count: files }),
    t("summary.commands", { count: commands }),
  ];
  if (run)
    counts.push(
      t("summary.input", { count: run.promptTokens }),
      t("summary.output", { count: run.completionTokens }),
    );
  const outcome =
    run?.status === "failed"
      ? {
          // Every failure reads as what happened and what to do (the owner's Mac test); the runtime's recorded
          // reason is a diagnostic, never shown, and an unknown code shows after "The run stopped".
          reason: t("summary.reason", {
            reason: failureReason(run, t) ?? t("summary.runFailed"),
          }),
          next: failureNext(run, t),
          provider: failureProvider(run),
          engine: failureEngine(run),
          code: failureCode(run),
        }
      : run?.status === "cancelled"
        ? {
            // "Stopped." alone: the composer already offers the next message (a review before 17.0, WEB-U-016).
            reason: t("summary.cancelled"),
            next: null,
            provider: null,
            engine: null,
            code: null,
          }
        : null;
  // A clear next step only (never "undo": there is no reliable one). One file is the footer's own
  // Open; among several, the chart and the documents it made open, and a changed test file offers every test.
  const testChanged = written.some(
    (path) => TEST_FILE.test(path) && !NOT_CODE.test(path),
  );
  // Only what the files panel shows as what it is: images, then documents and pages, then tables and sheets; "the
  // chart" only for one image named like a chart.
  const images = written.filter((path) => previewKind(path) === "image");
  const charts = images.filter((path) => /chart/i.test(fileName(path)));
  const rank = {
    image: 0,
    document: 1,
    page: 1,
    pdf: 1,
    text: 1,
    table: 2,
    sheet: 2,
  } as const;
  const opens: FollowUp[] =
    files > 1
      ? written
          .filter(shownInPanel)
          .sort((a, b) => rank[previewKind(a)] - rank[previewKind(b)])
          .map((path) => ({
            kind: "open",
            path,
            chart: charts.length === 1 && charts[0] === path,
          }))
      : [];
  // At most two, and Run all the tests always keeps its place: it is the one step that sends something. A live check
  // before 17.0: named Open buttons only when they cover every file the run changed, and then no Open changes;
  // otherwise one Open changes and no names.
  const room = testChanged ? 1 : 2;
  const covered = files > 1 && opens.length === files && files <= room;
  const followUps: FollowUp[] = [
    ...(covered ? opens : []),
    ...(testChanged
      ? [{ kind: "send", key: "followUp.runTests" } as const]
      : []),
  ];
  return {
    activityLabel,
    files,
    // The one file it wrote, named in the footer with Open, when the run's changes are just that file.
    wrote: files === 1 && written.length === 1 ? (written[0] ?? null) : null,
    // Whether that file is new or was there before, so the footer says as the step line does.
    wroteKind: created.size === 1 ? ("created" as const) : ("updated" as const),
    followUps,
    covered,
    // A turn its run didn't finish says nothing of files when it changed none: its failure says what happened, and
    // the files may have come in an earlier run (WEB-U-373, a lead whose team used its budget). Nor does a turn that
    // led the team and changed none itself: its helpers' changes are on their own runs, and the team's end note says
    // what they handed back (WEB-U-389, real-world use before 17.0: "No files changed" right above
    // "Handed back: a.txt, b.txt").
    summary:
      files === 0 &&
      (run?.status === "failed" ||
        run?.status === "cancelled" ||
        tools.some((tool) => LEADING.has(tool.name)))
        ? null
        : t("summary.filesChanged", { count: files }),
    details: counts.join(" · "),
    outcome,
  };
}
