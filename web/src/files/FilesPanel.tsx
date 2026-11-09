"use client";

// Its copy travels with the panel, which loads when wanted (WEB-U-176, WEB-U-178).
import "./changes-catalogue";

import { useQueryClient } from "@tanstack/react-query";
import { RefreshCw, X } from "lucide-react";
import { lazy, Suspense, useEffect, useRef, useState } from "react";

import { useSession } from "../auth/use-session";
import type { ContextState } from "../context/context-state";
import { useT } from "../i18n/I18nProvider";
import type { ReplayStep } from "../screen/replay-steps";
import type { ScreenNoticeView } from "../transcript/types";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";
import { Skeleton } from "../ui/Skeleton";
import { tabPanelIds } from "../ui/Tabs";

import type { RunChoice } from "./ChangesView";
import { FileList } from "./FileList";
import { FilePath } from "./FilePath";
import { PanelTabs, type PanelTab } from "./PanelTabs";
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
  screenSteps = [],
  runStarts = {},
  screenNotices = [],
  signInMayRemain = true,
  screenHandedBack = false,
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
  /** The task's desktop actions, for the Screen tab's replay. */
  screenSteps?: readonly ReplayStep[];
  /** When each run started, for the replay's run picker. */
  runStarts?: Readonly<Record<string, string>>;
  /** The task's live screen notices, for a sign-out's outcome (WEB-U-259). */
  screenNotices?: readonly ScreenNoticeView[];
  /** Whether the desktop's browser may hold a sign-in, so clearing it is offered; true when unknown. */
  signInMayRemain?: boolean;
  /** Handed back with no run since, from the notices: the Screen panel's status keeps its next step. */
  screenHandedBack?: boolean;
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
  // Escape anywhere inside the panel closes it (the bar's toggle then takes focus), except inside a dialog the panel
  // opened (a preview opened wide), which closes only itself.
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
      onClose();
    };
    element?.addEventListener("keydown", escape);
    return () => element?.removeEventListener("keydown", escape);
  }, [onClose]);
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
    >
      <div className="files-heading">
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
        <Button
          variant="ghost"
          className="icon-button"
          aria-label={t("files.close")}
          title={t("files.close")}
          onClick={onClose}
        >
          <Icon glyph={X} size={16} />
        </Button>
      </div>
      {current === "files" && (
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
        role="tabpanel"
        {...tabPanelIds(FILES_PANEL_ID, current)}
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
            <FilePreview
              conversationId={conversationId}
              path={file}
              onBack={() => setFile(null)}
            />
          </Suspense>
        )}
      </div>
    </aside>
  );
}

/** Joins a folder and a name into a workspace path ("" is the root). */
export const child = (folder: string, name: string) =>
  folder ? `${folder}/${name}` : name;
