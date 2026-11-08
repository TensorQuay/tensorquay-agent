import { english, type MessageKey, type Translate } from "../i18n/translate";

import { connectorName } from "./connector-call";
import { argument, bounded } from "./tool-results";
import { commandParts, literal, toolSummary } from "./tool-summary";
import type { ToolView } from "./types";
// Loaded as a run goes live (step-words-loader.ts), so its copy travels with it, outside the first scripts (WEB-U-014).
import "../i18n/deferred-catalogue";

type Words = (tool: ToolView, t: Translate) => string;

/** The runners whose run is the tests, as a command's first word. */
const RUNNERS = new Set([
  "pytest",
  "py.test",
  "jest",
  "vitest",
  "mocha",
  "rspec",
  "phpunit",
  "ctest",
  "tox",
  "nox",
]);
/** The tools whose `test` subcommand runs the tests. */
const TESTING = new Set([
  "cargo",
  "go",
  "dotnet",
  "mvn",
  "gradle",
  "gradlew",
  "mix",
  "swift",
  "zig",
  "deno",
]);
/** The package managers whose `test` script (or a runner they start) runs the tests. */
const SCRIPTS = new Set(["npm", "pnpm", "yarn", "bun"]);

/** Whether one simple command, as its words, starts a test run: a runner, a `test` subcommand or a package's test script. */
function testRun(words: readonly string[]): boolean {
  let rest = words;
  // An environment assignment before the command ("CI=1 make test").
  while (rest[0] !== undefined && /^[A-Za-z_]\w*=/.test(rest[0]))
    rest = rest.slice(1);
  const [first = "", ...after] = rest;
  const program = first.split("/").at(-1) ?? "";
  const args = after.filter((word) => !word.startsWith("-"));
  const from = (word: string) => testRun(after.slice(after.indexOf(word) + 1));
  if (RUNNERS.has(program)) return true;
  if (/^python[\d.]*$/.test(program)) {
    const at = after.indexOf("-m");
    return at >= 0 && ["pytest", "unittest"].includes(after[at + 1] ?? "");
  }
  if (["npx", "bunx", "uvx"].includes(program)) return testRun(after);
  if (["uv", "poetry", "pipenv", "hatch"].includes(program))
    return args[0] === "run" && from("run");
  if (SCRIPTS.has(program)) {
    if (args[0] === "exec" || args[0] === "dlx") return from(args[0]);
    const script = (args[0] === "run" ? args[1] : args[0]) ?? "";
    return /^test(?::|$)/.test(script) || RUNNERS.has(script);
  }
  if (program === "make") return args[0] === "test" || args[0] === "check";
  return (
    TESTING.has(program) &&
    (args[0] === "test" || (program === "cargo" && args[0] === "nextest"))
  );
}

/** Whether a command runs the tests: one of its simple commands (between `&&`, `;`, `|` or lines) starts a test run. */
export function runsTests(command: unknown): boolean {
  return (
    typeof command === "string" &&
    command
      .split(/&&|\|\||[;|\n]/)
      .some((part) => testRun(part.trim().split(/\s+/)))
  );
}

/** A file by its name alone: the call's own line, under the live line, names its whole path. */
function fileName(tool: ToolView, t: Translate): string {
  const path = argument(tool, "path");
  const name =
    typeof path === "string" ? path.split("/").filter(Boolean).at(-1) : "";
  return name ? bounded(name, "", 48) : t("step.aFile");
}
/** How much of a command the line says: its first words, its own "…" ending them. */
const COMMAND_LIMIT = 40;
/** The start of a one-line text, at most `limit` characters, cut without an ellipsis of its own. */
function clip(value: unknown, limit: number): string {
  if (typeof value !== "string") return "";
  return Array.from(value.replace(/\s+/g, " ").trim())
    .slice(0, limit)
    .join("")
    .trimEnd();
}
const helper = (tool: ToolView, key: string, t: Translate) =>
  bounded(argument(tool, key), t("tool.aHelper"), 48);
const plain =
  (key: MessageKey): Words =>
  (_tool, t) =>
    t(key);

/** The built-ins' present words, from the call's name and its first argument; never a tool's name. */
const WORDS: Readonly<Record<string, Words>> = {
  read_file: (tool, t) => t("step.read", { name: fileName(tool, t) }),
  write_file: (tool, t) => t("step.write", { name: fileName(tool, t) }),
  edit_file: (tool, t) => t("step.edit", { name: fileName(tool, t) }),
  // A test run is the tests, whatever runs them; another command by its first words, which its own line above names
  // whole; a script given inline by its language, as its line will.
  shell: (tool, t) => {
    const raw = argument(tool, "command");
    if (runsTests(raw)) return t("step.tests");
    const { line, lang } = commandParts(raw);
    if (lang) return t("step.script", { lang });
    const command = clip(line, COMMAND_LIMIT);
    return command ? t("step.command", { command }) : t("step.aCommand");
  },
  grep: (tool, t) => {
    const text = literal(argument(tool, "pattern"));
    return text ? t("step.lookFor", { text }) : t("step.lookForPattern");
  },
  computer: (tool, t) => {
    const action = argument(tool, "action");
    return action === "screenshot" || action === "observe"
      ? t("step.screen")
      : t("step.desktop");
  },
  read_notes: plain("step.notesRead"),
  write_notes: plain("step.notesWrite"),
  skill: (tool, t) =>
    t("step.skill", {
      name: bounded(argument(tool, "name"), t("tool.fallbackText"), 64),
    }),
  channel_post: plain("step.channelPost"),
  channel_read: plain("step.channelRead"),
  channel_wait: plain("step.channelWait"),
  task_claim: plain("step.taskClaim"),
  start_member: (tool, t) =>
    t("step.startHelper", { name: helper(tool, "role", t) }),
  assign: (tool, t) => t("step.giveWork", { name: helper(tool, "to", t) }),
  ask_all: plain("step.askAllHelpers"),
  decide: plain("step.tellDecision"),
  ask_person: plain("step.askYou"),
  send_to_lead: (tool, t) => {
    const kind = argument(tool, "type");
    return t("step.toLead", {
      kind: kind === "answer" || kind === "ask" ? kind : "report",
    });
  },
  take_files: plain("step.takeHelperFiles"),
  stop_member: (tool, t) =>
    t("step.stopHelper", { name: helper(tool, "name", t) }),
  read_helper: (tool, t) =>
    t("step.readHelper", { name: helper(tool, "name", t) }),
  wait_for_lead: plain("step.checkLead"),
  read_reports: plain("step.checkReports"),
  done: plain("step.wrapUpTeam"),
};
/** The calls whose own line already says what they do while they run: a listing, a page, a push. */
const SAID = new Set(["glob", "fetch_url", "git_push"]);

/**
 * The live line's words for the step a running call is (17.3): "Reading README.md…", "Running the tests…", "Looking at
 * the screen…". From the call's name and its first argument, never from the model's words; a connector's tool and a
 * tool the page doesn't know are named as their lines name them.
 */
export function stepWords(tool: ToolView, t: Translate = english): string {
  if (connectorName(tool.name) || SAID.has(tool.name))
    return toolSummary(tool, t);
  const words = Object.hasOwn(WORDS, tool.name) ? WORDS[tool.name] : undefined;
  return words
    ? words(tool, t)
    : t("step.tool", { name: bounded(tool.name, t("tool.fallbackTool"), 48) });
}
