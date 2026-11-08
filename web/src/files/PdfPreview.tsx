"use client";

import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";

import { api } from "../api/client";
import { useT } from "../i18n/I18nProvider";
import { formatNumber, formatSize } from "../i18n/message";
import type { MessageKey } from "../i18n/translate";
import { Skeleton } from "../ui/Skeleton";

import { pdfFailure, type PdfFailure } from "./pdf-failure";
import "./changes-catalogue";

type Drawn =
  | { state: "drawing" }
  | { state: "drawn"; pages: number; shown: number }
  | { state: "failed"; key: MessageKey };

/** The widest a page is drawn, in CSS pixels. */
const MAX_PAGE_WIDTH = 960;

const FAILURES: Record<PdfFailure, MessageKey> = {
  damaged: "files.pdfDamaged",
  protected: "files.pdfProtected",
  failed: "files.pdfFailed",
};

/**
 * A PDF's first pages, drawn by pdf.js in this page: the same in every browser, the PDF's own JavaScript never runs,
 * and only pages are drawn, so nothing in it is clickable. pdf.js loads only here (`./pdf-render`). A file over the
 * preview limit isn't fetched.
 */
export function PdfPreview({
  conversationId,
  path,
  name,
  size,
  etag,
  limit,
}: {
  conversationId: string;
  path: string;
  name: string;
  size: number;
  etag: string;
  limit: number;
}) {
  const t = useT();
  const small = size <= limit;
  const bytes = useQuery({
    queryKey: ["file-pdf", conversationId, path, etag],
    queryFn: async ({ signal }) =>
      (await api.fileBlob(conversationId, path, signal)).arrayBuffer(),
    enabled: small,
    retry: false,
    // Keyed by the version's etag, whose bytes never change: opening wide reuses them instead of fetching again.
    staleTime: Infinity,
  });
  const pages = useRef<HTMLDivElement>(null);
  // Each page's accessible name; kept in a ref so a new catalogue doesn't redraw the pages.
  const label = useRef((page: number) =>
    t("files.pdfPage", { page: String(page) }),
  );
  useEffect(() => {
    label.current = (page) => t("files.pdfPage", { page: String(page) });
  }, [t]);
  const [drawn, setDrawn] = useState<Drawn>({ state: "drawing" });
  const data = bytes.data;
  useEffect(() => {
    const into = pages.current;
    if (!data || !into) return;
    const stop = new AbortController();
    into.replaceChildren();
    void import("./pdf-render").then(async ({ renderPdf }) => {
      try {
        const done = await renderPdf(
          data,
          into,
          // A page is drawn at the view's width, up to a readable one when opened wide.
          Math.min(into.clientWidth || 320, MAX_PAGE_WIDTH),
          (page) => label.current(page),
          stop.signal,
        );
        if (!stop.signal.aborted) setDrawn({ state: "drawn", ...done });
      } catch (error) {
        if (!stop.signal.aborted)
          setDrawn({ state: "failed", key: FAILURES[pdfFailure(error)] });
      }
    });
    return () => {
      stop.abort();
      into.replaceChildren();
    };
  }, [data]);
  if (!small)
    return (
      <p className="type-small muted files-state">
        {t("files.pdfTooLarge", {
          size: formatSize(size, t.locale),
          limit: formatSize(limit, t.locale),
        })}
      </p>
    );
  if (bytes.isError)
    return (
      <p className="type-small muted files-state">{t("files.pdfFailed")}</p>
    );
  if (drawn.state === "failed")
    return <p className="type-small muted files-state">{t(drawn.key)}</p>;
  return (
    <div className="file-kind stack">
      {drawn.state === "drawn" && drawn.pages > drawn.shown && (
        <p className="type-small muted">
          {t("files.pdfPagesShown", {
            shown: formatNumber(drawn.shown, t.locale),
            count: formatNumber(drawn.pages, t.locale),
          })}
        </p>
      )}
      {(!data || drawn.state === "drawing") && (
        <Skeleton label={t("files.opening")} />
      )}
      <div
        ref={pages}
        className="file-pdf"
        role="region"
        aria-label={t("files.pdfPages", { name })}
        tabIndex={0}
      />
    </div>
  );
}
