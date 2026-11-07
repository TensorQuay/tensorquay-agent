import shapes from "../protocol/generated/key-shapes.json";

/**
 * Secret names that look like a key themselves (SEC-033): a key pasted where a name belongs. The page's mirror of the
 * gateway's `tqa_types::is_key_shaped_name`, reading the same rules (`key-shapes.json`, copied by protocol:gen and
 * checked for drift), so the page refuses what the gateway would before anything is sent (WEB-U-303). The rule is in
 * the file's `$comment`; kept conservative: a refused real name is worse than a missed key.
 */
type Prefix = {
  prefix: string;
  length: number;
  characters?: "token" | "upper_digit";
  change_percent?: number;
};
type Run = { length: number; classes: number; change_percent: number };
const RULES = shapes as { prefixes: Prefix[]; runs: Run[] };

/** A letter's or digit's class: digit, upper-case or lower-case; -1 for anything else. */
function charClass(char: string): number {
  if (char >= "0" && char <= "9") return 0;
  if (char >= "A" && char <= "Z") return 1;
  if (char >= "a" && char <= "z") return 2;
  return -1;
}

/** Whether `run` changes class at at least `percent` % of its neighbouring places. */
function changesOften(run: string, percent: number): boolean {
  const places = Math.max(run.length - 1, 0);
  let changes = 0;
  for (let index = 1; index < run.length; index++)
    if (charClass(run[index - 1]!) !== charClass(run[index]!)) changes++;
  return changes * 100 >= percent * places;
}

/** How many of the three classes `run` holds. */
const classCount = (run: string) =>
  new Set([...run].map(charClass).filter((found) => found >= 0)).size;

const TOKEN = /^[A-Za-z0-9_-]*/;
const UPPER_DIGIT = /^[A-Z0-9]*/;

function prefixed(name: string, rule: Prefix): boolean {
  if (!name.startsWith(rule.prefix)) return false;
  const rest = name.slice(rule.prefix.length);
  const body = (
    (rule.characters ?? "token") === "token" ? TOKEN : UPPER_DIGIT
  ).exec(rest)![0];
  const letters = body.replace(/[^A-Za-z0-9]/g, "");
  return (
    body.length >= rule.length &&
    changesOften(letters, rule.change_percent ?? 0)
  );
}

/** Whether `name` (with or without `secrets/`) looks like a key rather than a name, as the gateway decides it. */
export function isKeyShapedName(name: string): boolean {
  const bare = name.startsWith("secrets/")
    ? name.slice("secrets/".length)
    : name;
  if (RULES.prefixes.some((rule) => prefixed(bare, rule))) return true;
  return bare
    .split(/[^A-Za-z0-9]/)
    .some((run) =>
      RULES.runs.some(
        (rule) =>
          run.length >= rule.length &&
          classCount(run) >= rule.classes &&
          changesOften(run, rule.change_percent),
      ),
    );
}
