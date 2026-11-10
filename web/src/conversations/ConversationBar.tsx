"use client";

import { Pencil, Pin, Trash2 } from "lucide-react";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from "react";

import { useT } from "../i18n/I18nProvider";
import { NavigationButton } from "../shell/NavigationButton";
import { ActionMenu, type MenuAction } from "../ui/ActionMenu";
import { InlineEdit } from "../ui/InlineEdit";

import { useBarOffset } from "./use-bar-offset";

/**
 * One line above the transcript: title, run status in words, then the pickers. The title is renamed in place (the UI
 * standard's In-place edit), from a click on it or the pencil; a title that fails to save stays in the field.
 */
/** No extra actions: one list, so a bar without them keeps its props equal between renders. */
const NO_ACTIONS: readonly MenuAction[] = [];

export function ConversationBar({
  title,
  workspace,
  profile,
  origin,
  model,
  environment,
  status,
  change,
  onRename,
  onDelete,
  keepOn,
  extra = NO_ACTIONS,
  saving = false,
  choices,
  panel,
}: {
  title: string;
  /** The workspace picker, first among the pickers (new task only). */
  workspace?: ReactNode;
  /** The profile picker, after the workspace (new task only; ADR-0012). */
  profile?: ReactNode;
  /**
   * The quiet line under the title: the agent profile a task came from (ADR-0012) and the named workspace it is in,
   * "Agent: Analyst · In Team handbook".
   */
  origin?: string;
  model?: ReactNode;
  environment?: ReactNode;
  status?: ReactNode;
  /** Opens the task's model and environment (a conversation's header holds only its title and status). */
  change?: ReactNode;
  onRename?: (title: string) => Promise<boolean>;
  /** Asks to delete the task (the header's ⋯, beside Rename). */
  onDelete?: () => void;
  /**
   * The keep-alive pin (FR-33), the owner's: whether it is on, and a change to it. Its ⋯ item is a checked toggle, with
   * what it costs under it.
   */
  keepOn?: { on: boolean; onChange: (on: boolean) => void };
  /** More of the task's own actions, between Rename and Delete (a team's Stop, ADR-0034 S6). */
  extra?: readonly MenuAction[];
  saving?: boolean;
  /**
   * The task's model and environment, two small pickers right of the title and status (the Product Owner's correction,
   * WEB-U-113); icons alone where the header is narrow (WEB-U-106).
   */
  choices?: ReactNode;
  /** The right panel's toggle, last on the line. */
  panel?: ReactNode;
}) {
  const [editing, setEditing] = useState(false);
  const returnFocus = useRef(false);
  // The ⋯: the keyboard's way to rename, where focus returns after renaming.
  const more = useRef<HTMLButtonElement | null>(null);
  // After the edit ends, once the ⋯ is back in place (its ref is set only after the commit).
  useEffect(() => {
    if (editing || !returnFocus.current) return;
    returnFocus.current = false;
    more.current?.focus();
  }, [editing]);
  const t = useT();
  const main = useRef<HTMLDivElement>(null);
  // Where the header ends, for the files panel below it on a narrow column (WEB-U-171).
  const section = useRef<HTMLElement>(null);
  useBarOffset(section);
  const heading = useRef<HTMLHeadingElement>(null);
  const pickers = useRef<HTMLDivElement>(null);
  const place = useFieldPlace(editing, main, heading, pickers);
  const compact = useCompactChoices(main, pickers);
  const close = (focus: boolean) => {
    returnFocus.current = focus;
    setEditing(false);
  };
  return (
    <section
      ref={section}
      className="conversation-bar"
      aria-label={t("bar.region")}
      data-compact={compact || undefined}
    >
      <div className="conversation-bar-main" ref={main}>
        <NavigationButton />
        {/* While renaming, the title keeps its place (so the status and pickers stay put) and the field lies over it. */}
        {editing && onRename && (
          <InlineEdit
            value={title}
            label={t("title.label")}
            className="type-task-title conversation-title-field"
            style={place}
            onCancel={() => close(true)}
            onCommit={async (value) => {
              const saved = await onRename(value);
              // Back to the pencil after Enter; a click elsewhere keeps the focus it moved.
              if (saved)
                close(
                  document.activeElement?.classList.contains(
                    "conversation-title-field",
                  ) ?? false,
                );
              return saved;
            }}
          />
        )}
        <div className="conversation-title" data-editing={editing || undefined}>
          <div className="conversation-name">
            {/* A click on the title renames it where the name is; the ⋯'s Rename is the keyboard's way. */}
            {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events */}
            <h1
              ref={heading}
              className="type-task-title"
              title={title}
              data-renamable={onRename ? true : undefined}
              onClick={onRename && !saving ? () => setEditing(true) : undefined}
            >
              {title}
            </h1>
            {origin && (
              <span
                className="type-small muted conversation-origin"
                title={origin}
              >
                {origin}
              </span>
            )}
          </div>
        </div>
        {status}
        <div className="conversation-pickers" ref={pickers}>
          {workspace}
          {profile}
          {model}
          {environment}
          {change}
          {choices}
          {panel}
          {/* The UI standard's header: title, status, a quiet Files icon and one ⋯ (Rename, Keep environment on, a team's Stop, Delete task). */}
          {(onRename || onDelete || keepOn || extra.length > 0) && (
            <ActionMenu
              ref={more}
              label={t("list.more", { title })}
              actions={[
                ...(onRename
                  ? [
                      {
                        id: "rename",
                        label: t("task.rename"),
                        icon: Pencil,
                        onSelect: () => setEditing(true),
                      },
                    ]
                  : []),
                ...(keepOn
                  ? [
                      {
                        id: "keep",
                        label: t("task.keepOn"),
                        icon: Pin,
                        checked: keepOn.on,
                        note: t("task.keepOnNote"),
                        onSelect: () => keepOn.onChange(!keepOn.on),
                      },
                    ]
                  : []),
                ...extra,
                ...(onDelete
                  ? [
                      {
                        id: "delete",
                        label: t("task.delete"),
                        icon: Trash2,
                        danger: true,
                        onSelect: onDelete,
                      },
                    ]
                  : []),
              ]}
            />
          )}
        </div>
      </div>
    </section>
  );
}

/**
 * Where the title field lies while renaming: from the title's left edge to the pickers (or, when they wrap to their
 * own row on a phone, to the end of the bar), at the title's height, measured again when the window resizes.
 */
function useFieldPlace(
  editing: boolean,
  main: RefObject<HTMLDivElement | null>,
  heading: RefObject<HTMLHeadingElement | null>,
  pickers: RefObject<HTMLDivElement | null>,
): CSSProperties | undefined {
  const [place, setPlace] = useState<CSSProperties>();
  useLayoutEffect(() => {
    if (!editing) return;
    function measure() {
      const bar = main.current?.getBoundingClientRect();
      const title = heading.current?.getBoundingClientRect();
      if (!bar || !title) return;
      // What follows the title on its row: the pickers' own boxes, since on a phone their wrapper has none (display:
      // contents) and the choices take their own line (WEB-U-154).
      const onRow = Array.from(pickers.current?.children ?? [])
        .map((child) => child.getBoundingClientRect())
        .filter((box) => box.width > 0 && box.top < title.bottom);
      const rest = onRow.length
        ? onRow.reduce((first, box) => (box.left < first.left ? box : first))
        : undefined;
      const gap =
        parseFloat(
          getComputedStyle(main.current as Element).getPropertyValue(
            "--space-2",
          ),
        ) || 0;
      const sameRow = rest && rest.width > 0 && rest.top < title.bottom;
      const right = sameRow ? rest.left - gap : bar.right;
      setPlace({
        left: title.left - bar.left,
        top: title.top - bar.top,
        width: Math.max(right - title.left, title.width),
        height: title.height,
      });
    }
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [editing, main, heading, pickers]);
  return editing ? place : undefined;
}

/** The least room the title keeps beside the pickers' names (a few words), and the status (its word). */
const TITLE_LEAST = 128;
const STATUS_LEAST = 64;

/**
 * Whether the task's pickers must show as their icons (WEB-U-106): only when their names don't fit beside the title's
 * and the status's least room (WEB-U-125). The room the names need is measured while they show, again whenever a
 * name changes, and compared with the bar's width as it changes.
 */
function useCompactChoices(
  main: RefObject<HTMLDivElement | null>,
  pickers: RefObject<HTMLDivElement | null>,
): boolean {
  const [compact, setCompact] = useState(false);
  // The bar width the names need; null until measured with the names showing.
  const needed = useRef<number | null>(null);
  useLayoutEffect(() => {
    const bar = main.current;
    const rest = pickers.current;
    const choices = rest?.querySelector(".header-choices");
    if (!bar || !rest || !choices) return;
    function measure() {
      if (!bar || !rest) return;
      if (needed.current === null && !compact) {
        const gap = parseFloat(getComputedStyle(bar).columnGap) || 0;
        const others = [...bar.children].filter(
          (child) =>
            child instanceof HTMLElement &&
            !child.classList.contains("conversation-title") &&
            !child.classList.contains("conversation-status") &&
            getComputedStyle(child).position !== "absolute",
        ) as HTMLElement[];
        needed.current =
          others.reduce((sum, child) => sum + child.offsetWidth, 0) +
          TITLE_LEAST +
          STATUS_LEAST +
          gap * (others.length + 1);
      }
      if (needed.current !== null) setCompact(bar.clientWidth < needed.current);
    }
    measure();
    const sizes =
      typeof ResizeObserver === "function" ? new ResizeObserver(measure) : null;
    sizes?.observe(bar);
    // A name changed (another model chosen): measure again with the names showing.
    const names =
      typeof MutationObserver === "function"
        ? new MutationObserver(() => {
            needed.current = null;
            if (compact) setCompact(false);
            else measure();
          })
        : null;
    names?.observe(choices, {
      subtree: true,
      childList: true,
      characterData: true,
    });
    return () => {
      sizes?.disconnect();
      names?.disconnect();
    };
  }, [main, pickers, compact]);
  return compact;
}
