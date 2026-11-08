"use client";

import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";

import { api } from "../api/client";
import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import { DeferredMarkdown } from "../transcript/DeferredMarkdown";
import { Button } from "../ui/Button";
import { Skeleton } from "../ui/Skeleton";

import { parseTable } from "./csv";
import { extension, IMAGE_TYPES } from "./file-kind";
import "./changes-catalogue";

/** An image larger than this isn't fetched for a preview. */
export const MAX_IMAGE_BYTES = 10 * 1024 * 1024;
/** The rows a table shows; the count says how many there are. */
const MAX_ROWS = 200;
const SOURCE_KEY = "tensorquay-file-source";

/** A file's text with line numbers: the code view, and the source of a document or table on request. */
export function TextView({ name, text }: { name: string; text: string }) {
  const t = useT();
  const lines = text.replace(/\n$/, "").split("\n");
  return (
    <pre
      className="file-text type-code"
      role="region"
      aria-label={t("files.contents", { name })}
      tabIndex={0}
    >
      <code>
        {lines.map((line, index) => (
          <span className="file-line" key={index}>
            {line}
            {"\n"}
          </span>
        ))}
      </code>
    </pre>
  );
}

/** The bytes as base64, in chunks so a large file never overflows the call stack. */
function base64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  for (let start = 0; start < bytes.length; start += 0x8000)
    binary += String.fromCharCode(...bytes.subarray(start, start + 0x8000));
  return btoa(binary);
}

/**
 * An image drawn from its bytes, typed by its extension: the download is octet-stream and never rendered. The
 * query keeps the bytes; each mount makes and revokes its own object URL, so a remount never meets a revoked one. An
 * SVG shows from a data: URL instead: a blob: URL has the page's origin, and an SVG opened
 * from one in a new tab would run its script there, with the session. A data: document has an opaque origin, so even
 * if a browser opens it, its script can't reach this origin or the session: keep SVG off blob: URLs. A large image
 * isn't fetched.
 */
export function ImagePreview({
  conversationId,
  path,
  name,
  size,
  etag,
}: {
  conversationId: string;
  path: string;
  name: string;
  size: number;
  etag: string;
}) {
  const t = useT();
  const type = IMAGE_TYPES[extension(name)] ?? "application/octet-stream";
  const svg = type === "image/svg+xml";
  const small = size <= MAX_IMAGE_BYTES;
  const bytes = useQuery({
    queryKey: ["file-image", conversationId, path, etag],
    queryFn: async ({ signal }) =>
      (await api.fileBlob(conversationId, path, signal)).arrayBuffer(),
    enabled: small,
    retry: false,
  });
  const image = useRef<HTMLImageElement>(null);
  const data = bytes.data;
  const svgSource = useMemo(
    () => (svg && data ? `data:${type};base64,${base64(data)}` : undefined),
    [svg, data, type],
  );
  useEffect(() => {
    if (svg || !data || !image.current) return;
    const url = URL.createObjectURL(new Blob([data], { type }));
    image.current.src = url;
    return () => URL.revokeObjectURL(url);
  }, [svg, data, type]);
  if (!small)
    return (
      <p className="type-small muted files-state">
        {t("files.tooLarge", { size: formatSize(size, t.locale) })}
      </p>
    );
  if (bytes.isError)
    return (
      <p className="type-small muted files-state">{t("files.imageFailed")}</p>
    );
  if (!data) return <Skeleton label={t("files.opening")} />;
  return (
    // eslint-disable-next-line @next/next/no-img-element -- the workspace's own bytes (blob: or data:); next/image can't serve them
    <img ref={image} className="file-image" src={svgSource} alt={name} />
  );
}

function rememberedSource(): boolean {
  try {
    return localStorage.getItem(SOURCE_KEY) === "source";
  } catch {
    return false;
  }
}

/** A Markdown document rendered, its source a toggle away; the choice is remembered in this browser. */
export function DocumentPreview({
  name,
  text,
}: {
  name: string;
  text: string;
}) {
  const t = useT();
  const [source, setSource] = useState(rememberedSource);
  const flip = () => {
    const next = !source;
    setSource(next);
    try {
      localStorage.setItem(SOURCE_KEY, next ? "source" : "rendered");
    } catch {
      /* Storage may be unavailable; the choice then lasts for this view. */
    }
  };
  return (
    <div className="file-kind stack">
      <div>
        <Button variant="ghost" onClick={flip}>
          {t(source ? "files.showDocument" : "files.showSource")}
        </Button>
      </div>
      {source ? (
        <TextView name={name} text={text} />
      ) : (
        <div className="file-document">
          <DeferredMarkdown text={text} />
        </div>
      )}
    </div>
  );
}

/** A CSV or TSV file as a table: the first rows, how many there are, and the text on request. */
export function TablePreview({
  name,
  text,
  truncated,
}: {
  name: string;
  text: string;
  truncated: boolean;
}) {
  const t = useT();
  const [asText, setAsText] = useState(false);
  const rows = useMemo(
    () => parseTable(text, extension(name) === "tsv" ? "\t" : ",", truncated),
    [text, name, truncated],
  );
  if (!rows)
    return (
      <div className="file-kind stack">
        <p className="type-small muted">{t("files.notATable")}</p>
        <TextView name={name} text={text} />
      </div>
    );
  const [header = [], ...body] = rows;
  const shown = body.slice(0, MAX_ROWS);
  // A row wider than the header keeps its cells, under empty headings.
  const columns = Math.max(header.length, ...shown.map((row) => row.length));
  const count = truncated
    ? t("files.rowsFirst", { shown: String(shown.length) })
    : body.length > MAX_ROWS
      ? t("files.rowsShown", {
          shown: String(shown.length),
          count: String(body.length),
        })
      : t("files.rows", { count: body.length });
  return (
    <div className="file-kind stack">
      <div className="file-kind-bar">
        <span className="type-small muted">{count}</span>
        <Button variant="ghost" onClick={() => setAsText(!asText)}>
          {t(asText ? "files.showTable" : "files.showText")}
        </Button>
      </div>
      {asText ? (
        <TextView name={name} text={text} />
      ) : (
        <div
          className="file-table"
          role="region"
          aria-label={t("files.table", { name })}
          tabIndex={0}
        >
          <table aria-label={t("files.table", { name })}>
            <thead>
              <tr>
                {Array.from({ length: columns }, (_cell, index) => (
                  <th key={index} scope="col">
                    {header[index] ?? ""}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {shown.map((row, rowIndex) => (
                <tr key={rowIndex}>
                  {Array.from({ length: columns }, (_cell, index) => (
                    <td key={index}>{row[index] ?? ""}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
