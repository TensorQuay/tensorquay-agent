"use client";

// Its copy travels with the panel, which loads when wanted (WEB-U-176, WEB-U-178).
import "./changes-catalogue";

import { useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, RefreshCw, X } from "lucide-react";
import {
  lazy,
  Suspense,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
} from "react";

import { useSession } from "../auth/use-session";
import type { ContextState } from "../context/context-state";
import { useT } from "../i18n/I18nProvider";
import { desktopSteps } from "../screen/replay-steps";
import type { KeepSignIn } from "../screen/use-keep-sign-in";
import {
  desktopWordsState,
  onDesktopWords,
} from "../transcript/desktop-words-state";
import { toolSummary } from "../transcript/tool-summary";
import type { ScreenNoticeView, Transcript } from "../transcript/types";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";
import { Skeleton } from "../ui/Skeleton";
import { tabPanelIds } from "../ui/Tabs";

import type { RunChoice } from "./ChangesView";
import { FileList } from "./FileList";
import { FilePath } from "./FilePath";
import { PanelTabs, type PanelTab } from "./PanelTabs";
import {
  clampShare,
  DEFAULT_SHARE,
  keyedShare,
  rememberShare,
  rememberedShare,
  bounds,
} from "./split";
import { SplitContext, type SplitMode } from "./split-context";
import { FILES_PANEL_ID } from "./use-files-panel";
import type { FileTarget } from "./use-files-panel";
import { workspacePath } from "./workspace-path";

// The context and changes views, and a file's preview, load when first shown, outside the conversation page's first
// scripts (WEB-U-014).
const ScreenPanel = lazy(() =>
  import("../screen/ScreenPanel").then(({ ScreenPanel }) => ({
    default: ScreenPanel,
  })),
);
const ContextView = lazy(() =>
  import("../context/ContextView").then(({ ContextView }) => ({
    default: ContextView,
  })),
);
const ChangesView = lazy(() =>
  import("./ChangesView").then(({ ChangesView }) => ({ default: ChangesView })),
);
const FilePreview = lazy(() =>
  import("./FilePreview").then(({ FilePreview }) => ({ default: FilePreview })),
);

/**
 * The right panel (UI-GUIDELINES §6): one folder of the task's workspace at a time, or one file's preview. It refreshes
 * whenever the run changes (`version`), because the agent writes files while it works.
 */
export function FilesPanel({
  conversationId,
  running,
  version,
  runs,
  context,
  tab = "files",
  onTab,
  onClose,
  target = null,
  desktop = false,
  transcript = null,
  runStarts = {},
  screenNotices = [],
  signInMayRemain = true,
  screenHandedBack = false,
  keepSignIn = null,
  docks = true,
}: {
  conversationId: string;
  running: boolean;
  version: string;
  /** The task's runs in order, for the Changes tab. */
  runs: readonly RunChoice[];
  /** What the latest run loaded, for the Context tab (FR-11). */
  context: ContextState;
  tab?: PanelTab;
  onTab?: (tab: PanelTab) => void;
  onClose: () => void;
  /** A file to show, as asked for elsewhere (a finished turn's Open); a new request shows it again. */
  target?: FileTarget | null;
  /** Whether the task's environment declares a desktop: the Screen tab is offered then (ADR-0039 §5). */
  desktop?: boolean;
  /** The task's transcript: its desktop actions, in words, are the Screen tab's replay (ADR-0039 §5). */
  transcript?: Transcript | null;
  /** When each run started, for the replay's run picker. */
  runStarts?: Readonly<Record<string, string>>;
  /** The task's live screen notices, for a sign-out's outcome (WEB-U-259). */
  screenNotices?: readonly ScreenNoticeView[];
  /** Whether the desktop's browser may hold a sign-in, so clearing it is offered; true when unknown. */
  signInMayRemain?: boolean;
  /** Handed back with no run since, from the notices: the Screen panel's status keeps its next step. */
  screenHandedBack?: boolean;
  /** The task's "keep the sign-in" switch, for the Screen tab's ⋯ (ADR-0039 §6, amended for 18.0); null: not offered. */
  keepSignIn?: KeepSignIn | null;
  /**
   * Whether the task layout docks the panel beside the chat: a preview may then open as a split (WEB-U-494); below it
   * the panel takes the column and its way back is a back arrow.
   */
  docks?: boolean;
}) {
  const [localTab, setLocalTab] = useState(tab);
  // The Context tab is there only when the run loaded something; a view it was on falls back to Files.
  const available: readonly PanelTab[] = [
    "files",
    "changes",
    ...(context.kind === "report" ? (["context"] as const) : []),
    ...(desktop ? (["screen"] as const) : []),
  ];
  const chosen = onTab ? tab : localTab;
  const current = available.includes(chosen) ? chosen : "files";
  // The page has loaded the session already; the panel reads it and never asks again.
  const admin = useSession(false).data?.role === "admin";
  const setTab = onTab ?? setLocalTab;
  const [folder, setFolder] = useState("");
  const [file, setFile] = useState<string | null>(null);
  // A new target opens its folder and its preview (derived from the prop while rendering, not in an effect).
  const [shown, setShown] = useState(0);
  // A path outside the workspace opens nothing: the list says so instead (WEB-U-083).
  const [outside, setOutside] = useState(false);
  if (target && target.request !== shown) {
    setShown(target.request);
    setOutside(Boolean(target.outside));
    setFolder(
      !target.outside && target.path.includes("/")
        ? target.path.slice(0, target.path.lastIndexOf("/"))
        : "",
    );
    setFile(target.outside ? null : target.path);
  }
  const cache = useQueryClient();
  const aside = useRef<HTMLElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const moved = useRef(false);
  // The clicked entry leaves the page when a folder or file opens; focus stays in the panel instead of the page body.
  useEffect(() => {
    if (moved.current) body.current?.focus();
    moved.current = true;
  }, [folder, file]);
  const t = useT();
  // The split view (WEB-U-494): a preview opened wide beside the chat, its file's share remembered for this user.
  const user = useSession(false).data?.id ?? "";
  const [mode, setMode] = useState<SplitMode>("panel");
  const [share, setShare] = useState<number | null>(null);
  const [layoutWidth, setLayoutWidth] = useState(0);
  const splitting =
    docks && mode !== "panel" && current === "files" && file !== null;
  // Another file, another tab or the panel's own narrowing ends the split.
  const [splitFor, setSplitFor] = useState(file);
  if (splitFor !== file) {
    setSplitFor(file);
    setMode("panel");
  }
  useEffect(() => {
    const layout = aside.current?.parentElement;
    if (!layout) return;
    const measure = () => setLayoutWidth(layout.getBoundingClientRect().width);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(layout);
    return () => observer.disconnect();
  }, []);
  const fileShare = clampShare(share ?? rememberedShare(user), layoutWidth);
  const choose = (next: number) => {
    const rounded = Math.round(next * 1000) / 1000;
    setShare(rounded);
    rememberShare(user, rounded);
  };
  const split = {
    docks,
    mode: splitting ? mode : ("panel" as const),
    expand: () => setMode("split"),
    toggleFull: () => setMode((now) => (now === "full" ? "split" : "full")),
    close: () => setMode("panel"),
  };
  // Labelled again once the Desktop tool's words arrive (WEB-U-422); the words' summaries load with this panel, off
  // the task page's first scripts (WEB-U-473).
  const words = useSyncExternalStore(
    onDesktopWords,
    desktopWordsState,
    () => "waiting",
  );
  const screenSteps = useMemo(
    () =>
      transcript
        ? desktopSteps(transcript, (tool) => toolSummary(tool, t))
        : [],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [transcript, t, words],
  );
  // Escape anywhere inside the panel closes it (the bar's toggle then takes focus), except inside a dialog the panel
  // opened, which closes only itself; in the split it steps back once: full width to the split, the split to the
  // panel (WEB-U-494). The chat beside it is outside the panel, so its own Escape is never taken.
  const currentMode = split.mode;
  useEffect(() => {
    const element = aside.current;
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (
        event.target instanceof Element &&
        event.target.closest("dialog[open]")
      )
        return;
      event.stopPropagation();
      if (currentMode === "full") setMode("split");
      else if (currentMode === "split") setMode("panel");
      else onClose();
    };
    element?.addEventListener("keydown", escape);
    return () => element?.removeEventListener("keydown", escape);
  }, [onClose, currentMode]);
  const refresh = () =>
    Promise.all([
      cache.invalidateQueries({ queryKey: ["files", conversationId] }),
      cache.invalidateQueries({ queryKey: ["file", conversationId] }),
      cache.invalidateQueries({ queryKey: ["changes", conversationId] }),
      cache.invalidateQueries({ queryKey: ["diff", conversationId] }),
    ]);
  useEffect(() => {
    void cache.invalidateQueries({ queryKey: ["files", conversationId] });
    void cache.invalidateQueries({ queryKey: ["file", conversationId] });
    void cache.invalidateQueries({ queryKey: ["changes", conversationId] });
    void cache.invalidateQueries({ queryKey: ["diff", conversationId] });
  }, [cache, conversationId, version]);
  const open = (path: string) => {
    setOutside(false);
    setFile(null);
    setFolder(path);
  };
  return (
    // Its menus (the Changes tab's run picker) stay inside it, never over the transcript beside it (WEB-U-432).
    <aside
      className="files-panel"
      id={FILES_PANEL_ID}
      aria-label={t("files.title")}
      ref={aside}
      data-menu-bounds=""
      data-split={splitting ? mode : undefined}
      style={
        splitting
          ? ({ "--split-share": String(fileShare) } as CSSProperties)
          : undefined
      }
    >
      {splitting && mode === "split" && (
        <Divider share={fileShare} width={layoutWidth} onShare={choose} />
      )}
      {!splitting && (
        <div className="files-heading">
          {/* Below the breakpoint the panel takes the column: one way back to the chat (WEB-U-494). */}
          {!docks && (
            <Button
              variant="ghost"
              className="icon-button"
              aria-label={t("files.backToChat")}
              title={t("files.backToChat")}
              onClick={onClose}
            >
              <Icon glyph={ArrowLeft} size={16} />
            </Button>
          )}
          <PanelTabs current={current} onTab={setTab} shown={available} />
          {current !== "context" && current !== "screen" && (
            <Button
              variant="ghost"
              className="icon-button"
              aria-label={t("files.refresh")}
              title={t("files.refresh")}
              onClick={() => void refresh()}
            >
              <Icon glyph={RefreshCw} size={16} />
            </Button>
          )}
          {docks && (
            <Button
              variant="ghost"
              className="icon-button"
              aria-label={t("files.close")}
              title={t("files.close")}
              onClick={onClose}
            >
              <Icon glyph={X} size={16} />
            </Button>
          )}
        </div>
      )}
      {current === "files" && !splitting && (
        <FilePath folder={folder} file={file} onOpen={open} />
      )}
      {current === "files" && running && (
        <p className="type-small muted files-running">{t("files.running")}</p>
      )}
      <div
        className="files-body"
        ref={body}
        // A scrollable tab panel takes the focus so the keyboard can scroll it (WAI-ARIA tabs; axe
        // scrollable-region-focusable), as the panel below 1024 px often needs.
        // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
        tabIndex={0}
        // In the split the tabs give way to the file's header: no tab to label it then.
        {...(splitting
          ? { role: "region", "aria-label": file ?? undefined }
          : { role: "tabpanel", ...tabPanelIds(FILES_PANEL_ID, current) })}
      >
        {current === "screen" ? (
          <Suspense fallback={<Skeleton label={t("screen.connecting")} />}>
            <ScreenPanel
              conversationId={conversationId}
              working={running}
              steps={screenSteps}
              runStarts={runStarts}
              notices={screenNotices}
              signInMayRemain={signInMayRemain}
              keepSignIn={keepSignIn}
              handedBack={screenHandedBack}
            />
          </Suspense>
        ) : current === "context" ? (
          <Suspense fallback={<Skeleton label={t("context.loading")} />}>
            <ContextView state={context} showReasons={admin} />
          </Suspense>
        ) : current === "changes" ? (
          <Suspense fallback={<Skeleton label={t("changes.loading")} />}>
            <ChangesView
              conversationId={conversationId}
              runs={runs}
              running={running}
              onOpenFile={(raw) => {
                const path = workspacePath(raw);
                setOutside(path === null);
                setFolder(
                  path?.includes("/")
                    ? path.slice(0, path.lastIndexOf("/"))
                    : "",
                );
                setFile(path);
                setTab("files");
              }}
            />
          </Suspense>
        ) : file === null ? (
          <>
            {outside && (
              <p className="type-small muted" role="status">
                {t("files.outside")}
              </p>
            )}
            <FileList
              conversationId={conversationId}
              folder={folder}
              onFolder={open}
              onFile={(path) => {
                setOutside(false);
                setFile(path);
              }}
            />
          </>
        ) : (
          <Suspense fallback={<Skeleton label={t("files.opening")} />}>
            <SplitContext.Provider value={split}>
              <FilePreview
                conversationId={conversationId}
                path={file}
                onBack={() => setFile(null)}
              />
            </SplitContext.Provider>
          </Suspense>
        )}
      </div>
    </aside>
  );
}

/** Joins a folder and a name into a workspace path ("" is the root). */
export const child = (folder: string, name: string) =>
  folder ? `${folder}/${name}` : name;

/**
 * The split's divider (WEB-U-494): the panel's left edge, a vertical separator the pointer drags and the keys move
 * (arrows 2 %, Shift 10 %, Home and End to the limits, Enter back to 60 %; a double click too). Its value is the file's
 * share of the task layout, as a percentage.
 */
function Divider({
  share,
  width,
  onShare,
}: {
  share: number;
  width: number;
  onShare: (share: number) => void;
}) {
  const t = useT();
  const drag = useRef<{ x: number; share: number } | null>(null);
  const percent = Math.round(share * 100);
  const { least, most } = bounds(width || 1);
  return (
    // A focusable separator is a widget (WAI-ARIA's window splitter): it takes the focus and the keys.
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
    <div
      className="files-divider"
      role="separator"
      aria-orientation="vertical"
      aria-controls={FILES_PANEL_ID}
      aria-label={t("files.splitDivider")}
      aria-valuemin={Math.round(least * 100)}
      aria-valuemax={Math.round(most * 100)}
      aria-valuenow={percent}
      aria-valuetext={t("files.splitValue", {
        file: String(percent),
        chat: String(100 - percent),
      })}
      // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- the window splitter takes the focus
      tabIndex={0}
      onKeyDown={(event) => {
        const next = keyedShare(event.key, event.shiftKey, share, width);
        if (next === null) return;
        event.preventDefault();
        onShare(next);
      }}
      onDoubleClick={() => onShare(DEFAULT_SHARE)}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        event.currentTarget.setPointerCapture?.(event.pointerId);
        drag.current = { x: event.clientX, share };
      }}
      onPointerMove={(event) => {
        if (!drag.current || !(width > 0)) return;
        onShare(
          clampShare(
            drag.current.share + (drag.current.x - event.clientX) / width,
            width,
          ),
        );
      }}
      onPointerUp={() => {
        drag.current = null;
      }}
      onPointerCancel={() => {
        drag.current = null;
      }}
    />
  );
}
