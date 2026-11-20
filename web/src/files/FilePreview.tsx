"use client";

import { useQuery } from "@tanstack/react-query";
import { Download, Maximize2, MessageSquare, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { api } from "../api/client";
import { fileContent } from "../api/previews";
import { useSession } from "../auth/use-session";
import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";
import { Skeleton } from "../ui/Skeleton";

import { filesFailure } from "./file-errors";
import { previewKind } from "./file-kind";
import { MAX_IMAGE_BYTES } from "./image-bytes";
import { PagePreview } from "./PagePreview";
import { PdfPreview } from "./PdfPreview";
import {
  DocumentPreview,
  ImagePreview,
  TablePreview,
  TextView,
} from "./preview-kinds";
import { SheetPreview } from "./SheetPreview";
import { useSplit, type SplitMode } from "./split-context";
import "./changes-catalogue";

/** The gateway's preview bound; the note names it when a preview stops early. */
const PREVIEW_BYTES = 512 * 1024;
/** The gateway's default preview limits, until sign-in's own arrive (the gateway enforces them either way). */
const PREVIEW_LIMITS = {
  preview_html_bytes: 2 * 1024 * 1024,
  preview_pdf_bytes: 20 * 1024 * 1024,
  preview_sheet_bytes: 10 * 1024 * 1024,
};

/**
 * One file, shown as what it is: an image, a rendered document, a table, a web page in a sandboxed frame, a PDF's
 * pages, a spreadsheet's sheet, or text with line numbers; a plain note for other binary,
 * empty and cut-short files; always a download link.
 */
export function FilePreview({
  conversationId,
  path,
  onBack,
}: {
  conversationId: string;
  path: string;
  onBack: () => void;
}) {
  const t = useT();
  // Expand opens the file as a split beside the chat (WEB-U-494): the same preview, the panel made wider.
  const split = useSplit();
  const splitting = split.mode !== "panel";
  const heading = useRef<HTMLHeadingElement>(null);
  const toggle = useRef<HTMLButtonElement>(null);
  const expand = useRef<HTMLButtonElement>(null);
  const before = useRef<SplitMode>(split.mode);
  // What changed, said once: a name changing under the focus isn't always read again.
  const [status, setStatus] = useState("");
  // Focus follows each move: into the split, its name; between split and full width, the same button; back to the
  // panel, Expand.
  useEffect(() => {
    const from = before.current;
    before.current = split.mode;
    if (from === split.mode) return;
    if (from === "panel") heading.current?.focus();
    else if (split.mode === "panel") expand.current?.focus();
    else toggle.current?.focus();
    setStatus(
      split.mode === "full"
        ? t("files.chatHidden")
        : from === "full"
          ? t("files.chatBack")
          : "",
    );
  }, [split.mode, t]);
  const content = useQuery({
    queryKey: ["file", conversationId, path],
    queryFn: ({ signal }) => fileContent(api, conversationId, path, signal),
    retry: false,
  });
  // The page loaded the session before the panel; the preview only reads it.
  const limits = useSession(false).data?.limits ?? PREVIEW_LIMITS;
  const name = path.slice(path.lastIndexOf("/") + 1);
  if (content.isPending) return <Skeleton label={t("files.opening")} />;
  if (content.isError) {
    const failure = filesFailure(content.error, "files.goneFile");
    return (
      <div className="files-state stack">
        <p className="type-small muted">{t(failure.key)}</p>
        <Button variant="secondary" onClick={onBack}>
          {t("files.backToFolder")}
        </Button>
      </div>
    );
  }
  const file = content.data;
  const kind = previewKind(name);
  // A page, PDF, sheet or image over its preview limit shows only a note, so it has nothing to expand (a pass
  // over the pages before 17.0; WEB-U-255, and for an image WEB-U-517).
  const previewLimit =
    kind === "image"
      ? MAX_IMAGE_BYTES
      : kind === "page"
        ? limits.preview_html_bytes
        : kind === "pdf"
          ? limits.preview_pdf_bytes
          : kind === "sheet"
            ? limits.preview_sheet_bytes
            : null;
  // Every kind that previews opens wide, where the layout has room for the split; a note has nothing to widen.
  const wide =
    split.docks &&
    (previewLimit !== null
      ? file.size <= previewLimit
      : !file.binary && file.text !== null && file.text !== "");
  const view =
    kind === "image" ? (
      <ImagePreview
        conversationId={conversationId}
        path={path}
        name={name}
        size={file.size}
        etag={file.etag}
      />
    ) : kind === "page" ? (
      <PagePreview
        conversationId={conversationId}
        path={path}
        name={name}
        size={file.size}
        etag={file.etag}
        limit={limits.preview_html_bytes}
        text={file.truncated || file.binary ? null : file.text}
      />
    ) : kind === "pdf" ? (
      // A new version of the file is drawn afresh.
      <PdfPreview
        key={file.etag}
        conversationId={conversationId}
        path={path}
        name={name}
        size={file.size}
        etag={file.etag}
        limit={limits.preview_pdf_bytes}
      />
    ) : kind === "sheet" ? (
      <SheetPreview
        conversationId={conversationId}
        path={path}
        name={name}
        size={file.size}
        etag={file.etag}
        limit={limits.preview_sheet_bytes}
      />
    ) : file.binary || file.text === null ? (
      <p className="type-small muted files-state">{t("files.binary")}</p>
    ) : file.text === "" ? (
      <p className="type-small muted files-state">{t("files.emptyFile")}</p>
    ) : kind === "document" ? (
      <DocumentPreview name={name} text={file.text} />
    ) : kind === "table" ? (
      <TablePreview name={name} text={file.text} truncated={file.truncated} />
    ) : (
      <TextView name={name} text={file.text} />
    );
  return (
    <section className="file-preview" aria-label={name}>
      {/* One heading row, the panel's or the split's, then the one view: switching never remounts the view, so a
          page's scripts run once across Expand and back (WEB-U-494). */}
      <div className={splitting ? "split-heading" : "file-preview-heading"}>
        {splitting && (
          <h2 className="split-name" tabIndex={-1} ref={heading}>
            {name}
          </h2>
        )}
        {/* A sheet leads with its rows and columns (WEB-U-182); its size is no headline there. */}
        <span className="file-preview-size type-small muted">
          {kind === "sheet" ? null : formatSize(file.size, t.locale)}
        </span>
        <div className="file-preview-actions">
          {wide && !splitting && (
            <Button ref={expand} variant="ghost" onClick={split.expand}>
              <Icon glyph={Maximize2} size={16} />
              {t("files.expand")}
            </Button>
          )}
          <a
            className="button file-download"
            data-variant="secondary"
            href={api.downloadUrl(conversationId, path)}
            download={name}
          >
            <span className="button-label">
              <Icon glyph={Download} size={16} />
              {t("files.download")}
            </span>
          </a>
          {splitting && (
            <>
              <Button ref={toggle} variant="ghost" onClick={split.toggleFull}>
                <Icon
                  glyph={split.mode === "full" ? MessageSquare : Maximize2}
                  size={16}
                />
                {t(
                  split.mode === "full" ? "files.showChat" : "files.fullWidth",
                )}
              </Button>
              <Button
                variant="ghost"
                className="icon-button"
                aria-label={t("files.closeFile")}
                title={t("files.closeFile")}
                onClick={split.close}
              >
                <Icon glyph={X} size={16} />
              </Button>
            </>
          )}
        </div>
        {splitting && (
          <p className="sr-only" role="status">
            {status}
          </p>
        )}
      </div>
      {view}
      {file.truncated &&
        (kind === "document" || kind === "table" || kind === "text") && (
          <p className="type-small muted">
            {t("files.truncated", {
              size: formatSize(PREVIEW_BYTES, t.locale),
            })}
          </p>
        )}
    </section>
  );
}
