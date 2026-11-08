"use client";

import "../i18n/deferred-catalogue";

import {
  lazy,
  Suspense,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { useT } from "../i18n/I18nProvider";
import type { Conversation } from "../protocol/generated/api";
import { Dialog } from "../ui/Dialog";
import { Skeleton } from "../ui/Skeleton";

const TaskSearchResults = lazy(() =>
  import("./TaskSearchResults").then(({ TaskSearchResults }) => ({
    default: TaskSearchResults,
  })),
);

/** Typing settles this long before the search is sent. */
const SEARCH_DELAY_MS = 200;

/**
 * The search overlay (the design direction, sidebar): opened from the Search row or Ctrl/Cmd K, it searches every
 * task on the server; choosing a match, Escape or Close closes it and the list never changes under it.
 */
export function TaskSearch({
  onClose,
  row,
}: {
  onClose: () => void;
  /** A match's row; its link closes the overlay as it opens the task. */
  row: (
    conversation: Conversation,
    shared: boolean | undefined,
    onNavigate: () => void,
  ) => ReactNode;
}) {
  const t = useT();
  const box = useRef<HTMLInputElement>(null);
  const results = useRef<HTMLDivElement>(null);
  const matches = () => [
    ...(results.current?.querySelectorAll<HTMLAnchorElement>(
      "a.conversation-link",
    ) ?? []),
  ];
  // The arrow keys move through the matches, and up from the first returns to the box. A native listener, so the list
  // itself stays a plain list.
  useEffect(() => {
    const list = results.current;
    if (!list) return;
    const move = (event: KeyboardEvent) => {
      if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
      const links = matches();
      const at = links.indexOf(event.target as HTMLAnchorElement);
      if (at < 0) return;
      event.preventDefault();
      const next = at + (event.key === "ArrowDown" ? 1 : -1);
      if (next < 0) box.current?.focus();
      else links[Math.min(next, links.length - 1)]?.focus();
    };
    list.addEventListener("keydown", move);
    return () => list.removeEventListener("keydown", move);
  }, []);
  const [typed, setTyped] = useState("");
  const [query, setQuery] = useState("");
  useEffect(() => {
    const value = typed.trim();
    if (value === "") return;
    const timer = setTimeout(() => setQuery(value), SEARCH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [typed]);
  return (
    <Dialog
      open
      onClose={onClose}
      title={t("list.search")}
      className="task-search-dialog"
      initialFocus={box}
    >
      <input
        ref={box}
        type="search"
        className="form-control"
        aria-label={t("list.search")}
        placeholder={t("list.search")}
        value={typed}
        onChange={(event) => {
          setTyped(event.target.value);
          if (event.target.value.trim() === "") setQuery("");
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onClose();
          }
          // Enter opens the first match; down moves into the matches.
          if (event.key === "Enter") {
            event.preventDefault();
            matches()[0]?.click();
          }
          if (event.key === "ArrowDown") {
            event.preventDefault();
            matches()[0]?.focus();
          }
        }}
      />
      <div ref={results} className="task-search-results stack">
        {query && (
          <Suspense fallback={<Skeleton label={t("list.searching")} />}>
            <TaskSearchResults
              query={query}
              row={(conversation, shared) => row(conversation, shared, onClose)}
            />
          </Suspense>
        )}
      </div>
    </Dialog>
  );
}
