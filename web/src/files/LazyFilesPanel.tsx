"use client";

import { X } from "lucide-react";
import { lazy, Suspense, type ComponentProps } from "react";

import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";
import { PanelBoundary, PanelFailure } from "../ui/PanelBoundary";
import { Skeleton } from "../ui/Skeleton";

import { FILES_PANEL_ID } from "./use-files-panel";

/** The panel's code, fetched once; the panel starts closed, so it isn't part of the task page's first load (WEB-U-176). */
const load = () => import("./FilesPanel");
const lazyPanel = () =>
  lazy(() => load().then(({ FilesPanel }) => ({ default: FilesPanel })));
/** React keeps a failed load; Try again starts a new one (offline, then back online). */
let Panel = lazyPanel();

/**
 * Fetches the panel's code when the person is about to open it (the Files button under the pointer, pressed or
 * focused): it usually arrives before the click, and is never downloaded on a page that doesn't open the panel.
 */
export function preloadFilesPanel() {
  void load();
}

/**
 * The files panel, loaded when wanted. Until its code arrives the panel is already there: the same element, id, name
 * and place, marked busy, so opening it never flashes or shifts the page.
 */
export function LazyFilesPanel(props: ComponentProps<typeof Panel>) {
  const t = useT();
  return (
    <PanelBoundary
      // WEB-U-253: a panel that can't be shown keeps its frame, its name and its close.
      fallback={(retry) => (
        <aside
          className="files-panel"
          id={FILES_PANEL_ID}
          aria-label={t("files.title")}
        >
          <div className="files-heading">
            <h2 className="type-body-strong">{t("files.title")}</h2>
            <Button
              variant="ghost"
              className="icon-button"
              aria-label={t("files.close")}
              title={t("files.close")}
              onClick={props.onClose}
            >
              <Icon glyph={X} size={16} />
            </Button>
          </div>
          <div className="files-body files-state">
            <PanelFailure
              onRetry={() => {
                Panel = lazyPanel();
                retry();
              }}
            />
          </div>
        </aside>
      )}
    >
      <Suspense
        fallback={
          <aside
            className="files-panel"
            id={FILES_PANEL_ID}
            aria-label={t("files.title")}
            aria-busy="true"
          >
            <Skeleton label={t("skeleton.loading")} />
          </aside>
        }
      >
        <Panel {...props} />
      </Suspense>
    </PanelBoundary>
  );
}
