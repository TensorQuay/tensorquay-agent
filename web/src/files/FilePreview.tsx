"use client";

import { useQuery } from "@tanstack/react-query";
import { Download, Maximize2 } from "lucide-react";
import { useState } from "react";

import { api } from "../api/client";
import { fileContent } from "../api/previews";
import { useSession } from "../auth/use-session";
import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { Icon } from "../ui/Icon";
import { Skeleton } from "../ui/Skeleton";

import { filesFailure } from "./file-errors";
import { previewKind } from "./file-kind";
import { PagePreview } from "./PagePreview";
import { PdfPreview } from "./PdfPreview";
import {
  DocumentPreview,
  ImagePreview,
  TablePreview,
  TextView,
} from "./preview-kinds";
import { SheetPreview } from "./SheetPreview";
import "./changes-catalogue";

/** The gateway's preview bound (answers-068 Q4); the note names it when a preview stops early. */
const PREVIEW_BYTES = 512 * 1024;
/** The gateway's default preview limits, until sign-in's own arrive (the gateway enforces them either way). */
const PREVIEW_LIMITS = {
  preview_html_bytes: 2 * 1024 * 1024,
  preview_pdf_bytes: 20 * 1024 * 1024,
  preview_sheet_bytes: 10 * 1024 * 1024,
};

/**
 * One file, shown as what it is (findings F1-F3; the file previews design): an image, a rendered document, a table, a
 * web page in a sandboxed frame, a PDF's pages, a spreadsheet's sheet, or text with line numbers; a plain note for other binary,
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
  // A page, a PDF or a sheet opens wide on request, over the conversation (Escape closes it).
  const [expanded, setExpanded] = useState(false);
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
  // A page, PDF or sheet opens wide; one over its preview limit shows only a note, so it has nothing to expand (a pass
  // over the pages before 17.0; WEB-U-255).
  const previewLimit =
    kind === "page"
      ? limits.preview_html_bytes
      : kind === "pdf"
        ? limits.preview_pdf_bytes
        : kind === "sheet"
          ? limits.preview_sheet_bytes
          : null;
  const wide = previewLimit !== null && file.size <= previewLimit;
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
      <div className="file-preview-heading">
        {/* A sheet leads with its rows and columns (WEB-U-182); its size is no headline there. */}
        <span className="type-small muted">
          {kind === "sheet" ? null : formatSize(file.size, t.locale)}
        </span>
        <div className="file-preview-actions">
          {wide && (
            <Button variant="ghost" onClick={() => setExpanded(true)}>
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
        </div>
      </div>
      {/* One view at a time: a page's scripts never run twice. */}
      {!expanded && view}
      {wide && (
        <Dialog
          open={expanded}
          title={name}
          className="preview-dialog"
          onClose={() => setExpanded(false)}
        >
          {expanded && view}
        </Dialog>
      )}
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
