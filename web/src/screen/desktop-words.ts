/**
 * The Desktop tool's step words (ADR-0039 §5): a `computer` action in words and the element it acted on. Only a
 * Desktop task shows them, so they travel with the screen area, off the conversation page's first load (WEB-U-422):
 * transcript/tool-summary.ts loads this module as a Desktop task's page opens, or at its first `computer` step.
 */
import { formatDuration } from "../i18n/message";
import type { MessageKey, Translate } from "../i18n/translate";
import type { ScreenRole } from "../protocol/generated/protocol";
import { argument, bounded } from "../transcript/tool-results";
import type { ToolView } from "../transcript/types";
import "./catalogue";

/** An unnamed element's role in words (revision 14's closed ScreenRole): a new role fails tsc until it has words. */
const ROLE_WORDS: Readonly<Record<Exclude<ScreenRole, "other">, MessageKey>> = {
  button: "screen.roleButton",
  link: "screen.roleLink",
  textbox: "screen.roleTextbox",
  searchbox: "screen.roleSearchbox",
  checkbox: "screen.roleCheckbox",
  radio: "screen.roleRadio",
  switch: "screen.roleSwitch",
  combobox: "screen.roleCombobox",
  listbox: "screen.roleListbox",
  option: "screen.roleOption",
  menu: "screen.roleMenu",
  menuitem: "screen.roleMenuitem",
  menuitemcheckbox: "screen.roleMenuitemcheckbox",
  menuitemradio: "screen.roleMenuitemradio",
  tab: "screen.roleTab",
  slider: "screen.roleSlider",
  spinbutton: "screen.roleSpinbutton",
  treeitem: "screen.roleTreeitem",
  cell: "screen.roleCell",
  icon: "screen.roleIcon",
  heading: "screen.roleHeading",
};

/**
 * The element a desktop action acted on, as a line says it (ADR-0039 §5, revision 14's target): its name, quoted;
 * without one, its role in words; otherwise null, and the line says "on the screen". Screen text, shown as text only.
 */
function element(tool: ToolView, t: Translate): string | null {
  const target = tool.target;
  if (!target) return null;
  const name = bounded(target.name, "", 60);
  if (name) return t("screen.named", { name });
  return target.role === "other" ? null : t(ROLE_WORDS[target.role]);
}

/**
 * A desktop step's own output as a person reads it (WEB-U-368): its first line names the element it acted on by the
 * model's ref ("Typed 17 characters into e8", "Clicked mark 7 (e7)"); that ref becomes the element as the step's title
 * names it, or "an element". Only the first line: a snapshot after it lists every element by its ref.
 */
export function namedRef(text: string, tool: ToolView, t: Translate): string {
  if (tool.name !== "computer") return text;
  const ref = argument(tool, "ref");
  if (typeof ref !== "string" || !/^e?\d{1,4}$/.test(ref)) return text;
  // As the tool shows it (tqa-tools snapshot.rs `shown`): a mark's number as "mark 7 (e7)", a ref as itself.
  const shown = /^\d/.test(ref) ? `mark ${ref} (e${ref})` : ref; // copy-lint: not user copy
  const end = text.indexOf("\n");
  const first = end === -1 ? text : text.slice(0, end);
  const named = element(tool, t) ?? t("screen.someElement");
  const escaped = shown.replace(/[()]/g, "\\$&");
  const replaced = first.replace(
    new RegExp(`(^|\\s)${escaped}(?=$|[\\s;,.])`),
    `$1${named}`,
  );
  return end === -1 ? replaced : replaced + text.slice(end);
}

/**
 * A `computer` action in words (ADR-0039 §5: "Clicked “Save”", "Typed 42 characters", "Pressed ctrl+s"), from its
 * arguments and the element it acted on; never a ref, a coordinate or the tool's name. A refusal the person would want
 * to know of reads as what happened.
 */
export function computer(tool: ToolView, t: Translate): string {
  const refused = tool.failure?.code;
  if (refused === "stale_ref") return t("tool.computerStale");
  if (refused === "screenshot_limit") return t("tool.computerScreenshotLimit");
  if (refused === "action_limit") return t("tool.computerActionLimit");
  if (refused === "screenshot_too_large")
    return t("tool.computerScreenshotTooLarge");
  const action = argument(tool, "action");
  const on = element(tool, t);
  switch (action) {
    case "observe":
      return t("tool.computerObserve");
    case "screenshot":
      return t("tool.computerScreenshot");
    case "click":
    case "double_click":
      return on
        ? t("tool.computerClickOn", { kind: action, element: on })
        : t("tool.computerClick", { kind: action });
    case "type": {
      const text = argument(tool, "text");
      const count = typeof text === "string" ? Array.from(text).length : 0;
      return on
        ? t("tool.computerTypeInto", { count, element: on })
        : t("tool.computerType", { count });
    }
    case "key": {
      const keys = argument(tool, "keys");
      return t("tool.computerKey", {
        keys: bounded(keys, t("tool.fallbackText"), 40),
      });
    }
    case "scroll": {
      const raw = argument(tool, "direction");
      const direction =
        raw === "up" || raw === "left" || raw === "right" ? raw : "down";
      return on
        ? t("tool.computerScrollIn", { direction, element: on })
        : t("tool.computerScroll", { direction });
    }
    case "drag":
      return t("tool.computerDrag");
    case "wait": {
      const ms = argument(tool, "ms");
      return t("tool.computerWait", {
        time: formatDuration(
          typeof ms === "number" ? Math.round(ms / 100) / 10 : 0,
          "second",
          t.locale,
        ),
      });
    }
    default:
      return t("tool.computerOther");
  }
}
