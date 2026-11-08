import { workspacePath } from "../files/workspace-path";
import { english, type Translate } from "../i18n/translate";

import { connectorName } from "./connector-call";
import { blockedAccess } from "./egress";
import { noticeText } from "./notice-copy";
import {
  argument,
  bounded,
  fileChange,
  readRange,
  searchCount,
} from "./tool-results";
import type { ToolView } from "./types";

type Summary = (tool: ToolView, t: Translate) => string;

/** The start of a one-line text, cut at `limit` characters with "…" (a command's first words, a page's address). */
function leading(value: unknown, fallback: string, limit: number): string {
  const text =
    typeof value === "string" ? value.replace(/\s+/g, " ").trim() : "";
  if (!text) return fallback;
  const points = Array.from(text);
  return points.length <= limit
    ? text
    : `${points.slice(0, limit - 1).join("")}…`;
}

/** A page by its site and path ("example.com/pricing"), without "www.", the query or the fragment. */
function page(url: unknown): string | null {
  if (typeof url !== "string" || !URL.canParse(url)) return null;
  const { hostname, pathname } = new URL(url);
  const path = pathname === "/" ? "" : pathname.replace(/\/$/, "");
  return leading(`${hostname.replace(/^www\./, "")}${path}`, "", 60);
}

function change(tool: ToolView, t: Translate): string {
  const changed = fileChange(tool);
  return changed
    ? t("tool.change", { added: changed.added, removed: changed.removed })
    : "";
}
function count(tool: ToolView, t: Translate): string {
  const found = searchCount(tool);
  if (!found) return "";
  return (
    t("tool.count", { count: found.count, unit: found.unit }) +
    (found.partial ? t("tool.partial") : "")
  );
}
/** The file a tool names, as the files panel names it: "/workspace/x" is x (WEB-U-083); another path stays as given. */
const filePath = (tool: ToolView, t: Translate) => {
  const raw = argument(tool, "path");
  return bounded(
    typeof raw === "string" ? (workspacePath(raw) ?? raw) : raw,
    t("tool.fallbackFile"),
  );
};
const writeFile: Summary = (tool, t) => {
  const changed = fileChange(tool);
  const path = filePath(tool, t);
  return (
    t("tool.write", { kind: changed?.kind ?? "none", path }) + change(tool, t)
  );
};
const editFile: Summary = (tool, t) =>
  t("tool.edit", {
    done: tool.status === "ok" ? "yes" : "no",
    path: filePath(tool, t),
  }) + change(tool, t);
const readFile: Summary = (tool, t) => {
  const range = readRange(tool);
  const path = filePath(tool, t);
  return (
    t("tool.read", { path }) +
    (range
      ? range.first === range.last
        ? t("tool.readLine", { first: range.first })
        : t("tool.readLines", range)
      : "")
  );
};
/**
 * The words a regular expression looks for: its longest literal run, with escapes, classes, anchors and quantifiers
 * taken out; null when there is none worth saying.
 */
export function literal(pattern: unknown): string | null {
  if (typeof pattern !== "string") return null;
  const text = pattern
    .replace(/\\[dDwWsSbB]/g, " ")
    .replace(/\[[^\]]*\]/g, " ")
    .replace(/\\(.)/g, "$1")
    .replace(/[\^$.*+?()[\]{}|]/g, " ");
  const runs =
    text.match(
      /[\p{L}\p{N}][\p{L}\p{N} _'-]*[\p{L}\p{N}]|[\p{L}\p{N}]{2,}/gu,
    ) ?? [];
  const longest = runs.sort((a, b) => b.length - a.length)[0];
  return longest ? bounded(longest, "", 40) : null;
}
/** The extensions a listing names as a kind of file (the fileKind message); any other says only "files". */
const KINDS: ReadonlySet<string> = new Set([
  "csv",
  "tsv",
  "md",
  "json",
  "pdf",
  "py",
  "rs",
  "ts",
  "tsx",
  "js",
  "go",
  "png",
  "jpg",
  "jpeg",
  "svg",
  "html",
  "yaml",
  "yml",
]);
// The words it looked for, where; the expression itself stays in the card.
const grep: Summary = (tool, t) => {
  const words = literal(argument(tool, "pattern"));
  const path = argument(tool, "path");
  const values = {
    text: words ?? "",
    path:
      typeof path === "string" && path && path !== "."
        ? bounded(path, "", 48)
        : t("tool.theWorkspace"),
  };
  // As a running listing's line does, a running search says what it looks for; done, what it looked for (17.3).
  if (tool.status === "running")
    return words
      ? t("tool.lookingForText", values)
      : t("tool.lookingForPattern", values);
  return (
    (words ? t("tool.lookedFor", values) : t("tool.lookedForPattern", values)) +
    count(tool, t)
  );
};
// How many files of a kind it found; the glob itself stays in the card.
const glob: Summary = (tool, t) => {
  const pattern = argument(tool, "pattern");
  const extension =
    typeof pattern === "string"
      ? /\*\.([A-Za-z0-9]+)$/.exec(pattern)?.[1]?.toLowerCase()
      : undefined;
  const kind =
    extension && KINDS.has(extension)
      ? t("fileKind", { ext: extension })
      : undefined;
  const found = searchCount(tool);
  if (!found)
    return kind ? t("tool.lookingForKind", { kind }) : t("tool.lookingFor");
  return (
    (kind
      ? t("tool.foundKind", { count: found.count, kind })
      : t("tool.found", { count: found.count })) +
    (found.partial ? t("tool.partial") : "")
  );
};
// WEB-U-390: a finished script given inline (a heredoc, or a multi-line -c or -e, to an interpreter) is named by its
// language; any other multi-line command, or a script still running, shows its first line that isn't blank, a comment
// or `set …`, without a heredoc's marker, and "…". "a command" and its first words; a non-zero exit
// "didn't succeed", its code in the card.
/**
 * A command as its line says it (WEB-U-390): without the workspace's `cd`; of several lines, the first that isn't blank,
 * a comment or `set …`, without a heredoc's marker; and the language of a script given inline to an interpreter.
 */
export function commandParts(raw: unknown): {
  line: unknown;
  many: boolean;
  lang: string | undefined;
} {
  const text =
    typeof raw === "string"
      ? raw.replace(/^\s*cd \/workspace\s*&&\s*/, "")
      : raw;
  const lines = typeof text === "string" ? text.trim().split("\n") : [];
  const [head = ""] = lines;
  const many = lines.length > 1;
  const lang = many
    ? /^(python|node|bash|sh)[\d.]*\b.*(<<-?\s*\S+|\s-[ce]\s+['"])\s*$/.exec(
        head,
      )?.[1]
    : undefined;
  const line = many
    ? (lines.find((line) => !/^\s*(#|set\s|$)/.test(line)) ?? head).replace(
        /\s*<<-?\s*\S+\s*$/,
        "",
      )
    : text;
  return { line, many, lang };
}
const shell: Summary = (tool, t) => {
  const { line, many, lang } = commandParts(argument(tool, "command"));
  let command = leading(line, t("tool.fallbackCommand"), 60);
  // "Running …" ends in its own "…".
  if (tool.status === "running") return t("tool.running", { command });
  if (many && !command.endsWith("…")) command += " …";
  // A command stopped part way says so, not "Ran … Cancelled".
  if (tool.status === "cancelled" && tool.ran)
    return t("tool.stopped", { command });
  const ran = tool.ran ? "yes" : "no";
  return (
    (lang ? t("tool.script", { ran, lang }) : t("tool.ran", { ran, command })) +
    // A failed step's "Failed" mark says it (WEB-U-180); only a non-zero exit without that mark says it here.
    (tool.exitCode && tool.status !== "error" ? t("tool.didNotSucceed") : "")
  );
};
// The page it read, never the tool's name; a refusal keeps the allow-list's words, read from the call's
// failure, never from the result's words (C1, WEB-U-126).
const fetchUrl: Summary = (tool, t) => {
  const target =
    tool.status === "error" && tool.failure?.code === "egress_denied"
      ? (tool.failure.target ?? null)
      : null;
  if (target) return noticeText(blockedAccess(target), t);
  const values = {
    page: page(argument(tool, "url")) ?? t("tool.fallbackPage"),
  };
  if (tool.status === "running") return t("tool.fetchReading", values);
  return t(
    tool.status === "error" ? "tool.fetchFailed" : "tool.fetchRead",
    values,
  );
};
// The other built-ins in plain words. A channel tool names the channel's id, not its name, and the task
// page doesn't load the channel, so it says "the channel".
const plain =
  (
    key:
      | "tool.notesRead"
      | "tool.notesWrite"
      | "tool.channelPost"
      | "tool.channelRead"
      | "tool.channelWait"
      | "tool.askedAllHelpers"
      | "tool.toldDecision"
      | "tool.askedYou"
      | "tool.tookHelperFiles"
      | "tool.checkedLead"
      | "tool.checkedReports"
      | "tool.wrappedUpTeam",
  ): Summary =>
  (_tool, t) =>
    t(key);
const skill: Summary = (tool, t) =>
  t("tool.skill", {
    name: bounded(argument(tool, "name"), t("tool.fallbackText"), 64),
  });
const taskClaim: Summary = (tool, t) => {
  const number = argument(tool, "task_seq");
  return typeof number === "number"
    ? t("tool.taskClaim", { number: String(number) })
    : t("tool.taskClaimAny");
};
/** A project by its site and path ("example.com/team/report"), without ".git"; the owner's direction (c8). */
export function project(remote: unknown): string | null {
  return typeof remote === "string"
    ? page(remote.replace(/\.git\/?$/, ""))
    : null;
}
/** A push by the project it saves changes to, never its branch or the word push (the owner's direction, c8). */
const gitPush: Summary = (tool, t) => {
  const values = {
    project: project(argument(tool, "remote")) ?? t("tool.fallbackProject"),
  };
  if (tool.status === "running") return t("tool.pushing", values);
  return t(tool.status === "ok" ? "tool.pushed" : "tool.push", values);
};
/** A connector's tool on its connector: running, used, or (prepared, failed) as the plain intent (ADR-0031). */
function connectorCall(
  tool: ToolView,
  names: { connector: string; tool: string },
  t: Translate,
): string {
  const values = {
    tool: bounded(names.tool, t("tool.fallbackTool"), 64),
    connector: bounded(names.connector, t("tool.fallbackTool"), 32),
  };
  if (tool.status === "running") return t("tool.connectorUsing", values);
  return t(
    tool.status === "ok" ? "tool.connectorUsed" : "tool.connectorCall",
    values,
  );
}
// The agent-team tools in plain words (ADR-0034; a pass over the pages before 17.0): what the lead or helper did,
// naming the helper where the call does, never the tool's name.
const helper = (tool: ToolView, key: string, t: Translate) =>
  bounded(argument(tool, key), t("tool.aHelper"), 48);
// A start that failed never says "Started" beside its Failed mark (WEB-U-399, real-world use before 17.0).
const startMember: Summary = (tool, t) =>
  t(tool.status === "error" ? "tool.startHelperFailed" : "tool.startedHelper", {
    name: helper(tool, "role", t),
  });
const assign: Summary = (tool, t) =>
  t("tool.gaveWork", { name: helper(tool, "to", t) });
const stopMember: Summary = (tool, t) =>
  t("tool.stoppedHelper", { name: helper(tool, "name", t) });
const readHelper: Summary = (tool, t) =>
  t("tool.readHelper", { name: helper(tool, "name", t) });
const sendToLead: Summary = (tool, t) => {
  const kind = argument(tool, "type");
  return t("tool.sentToLead", {
    kind: kind === "answer" || kind === "ask" ? kind : "report",
  });
};

/**
 * The Desktop tool's step words (src/screen/desktop-words.ts, WEB-U-422): loaded as a Desktop task's page opens, or at
 * the first `computer` step anywhere; until then a step reads "Used the desktop", and a page that knows it is a
 * Desktop task waits for them before its steps render (LiveConversation).
 */
let desktopWords: typeof import("../screen/desktop-words") | null = null;
let desktopLoading: Promise<void> | null = null;
let desktopFailed = false;
const desktopArrivals = new Set<() => void>();

export function loadDesktopWords(): Promise<void> {
  // The server renders without them, as hydration's first render does (as the failure copy, WEB-U-392).
  if (typeof window === "undefined") return Promise.resolve();
  desktopLoading ??= import("../screen/desktop-words").then(
    (loaded) => {
      desktopWords = loaded;
      desktopFailed = false;
      for (const listener of desktopArrivals) listener();
    },
    () => {
      // A chunk that failed (a tab open across a deploy): the page shows its steps with the general label, and the
      // next computer step's label asks again.
      desktopLoading = null;
      desktopFailed = true;
      for (const listener of desktopArrivals) listener();
    },
  );
  return desktopLoading;
}

/** Whether the Desktop tool's words are here, failed to load, or are on their way; `onDesktopWords` calls back on
 * each change. */
export const desktopWordsState = () =>
  desktopWords ? "ready" : desktopFailed ? "failed" : "waiting";
export function onDesktopWords(listener: () => void): () => void {
  desktopArrivals.add(listener);
  return () => desktopArrivals.delete(listener);
}

/** A desktop step's own output with its element named (WEB-U-368), once the words are here; the text as it is before. */
export const namedRef = (text: string, tool: ToolView, t: Translate) =>
  desktopWords ? desktopWords.namedRef(text, tool, t) : text;

const computer: Summary = (tool, t) => {
  if (desktopWords) return desktopWords.computer(tool, t);
  void loadDesktopWords();
  return t("tool.computerOther");
};
const summaries: Readonly<Record<string, Summary>> = {
  write_file: writeFile,
  edit_file: editFile,
  read_file: readFile,
  grep,
  glob,
  shell,
  fetch_url: fetchUrl,
  git_push: gitPush,
  read_notes: plain("tool.notesRead"),
  write_notes: plain("tool.notesWrite"),
  skill,
  channel_post: plain("tool.channelPost"),
  channel_read: plain("tool.channelRead"),
  channel_wait: plain("tool.channelWait"),
  task_claim: taskClaim,
  start_member: startMember,
  assign,
  ask_all: plain("tool.askedAllHelpers"),
  decide: plain("tool.toldDecision"),
  ask_person: plain("tool.askedYou"),
  send_to_lead: sendToLead,
  take_files: plain("tool.tookHelperFiles"),
  stop_member: stopMember,
  read_helper: readHelper,
  wait_for_lead: plain("tool.checkedLead"),
  read_reports: plain("tool.checkedReports"),
  done: plain("tool.wrappedUpTeam"),
  computer,
};

/** A line's tooltip where the line shortens something: the page's whole address. */
export function toolTitle(tool: ToolView): string | null {
  if (tool.name !== "fetch_url") return null;
  const url = argument(tool, "url");
  return typeof url === "string" ? url : null;
}

/** Gives every builtin a bounded activity line in the reader's language; unknown tools reveal only their name. */
export function toolSummary(tool: ToolView, t: Translate = english): string {
  const connector = connectorName(tool.name);
  if (connector) return bounded(connectorCall(tool, connector, t), "", 180);
  const summary = Object.hasOwn(summaries, tool.name)
    ? summaries[tool.name]
    : undefined;
  const text = summary
    ? summary(tool, t)
    : t("tool.used", { name: bounded(tool.name, t("tool.fallbackTool")) });
  return bounded(text, "", 180);
}
