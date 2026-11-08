"use client";

import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useLayoutEffect, useRef, useState, type RefObject } from "react";

import { api } from "../api/client";
import { ApiError } from "../api/error";
import { fileSheet } from "../api/previews";
import { useT } from "../i18n/I18nProvider";
import { formatNumber, formatSize } from "../i18n/message";
import type { MessageKey } from "../i18n/translate";
import type { SheetPreview as Sheet } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { MenuButton } from "../ui/MenuButton";
import { Skeleton } from "../ui/Skeleton";

import "./changes-catalogue";

/** From this many sheets the picker is a list, not a row of buttons (§7 List tools). */
const PICKER_LIST = 8;

/** A column's spreadsheet name: A, B, … Z, AA, AB, … */
function columnName(index: number): string {
  let name = "";
  for (let rest = index; rest >= 0; rest = Math.floor(rest / 26) - 1)
    name = String.fromCharCode(65 + (rest % 26)) + name;
  return name;
}

/** A cell that reads as a number (1 204, -3.5, 12%), aligned right as a spreadsheet shows it. */
const NUMBER = /^[-+]?[\d\s.,]*\d%?$/;

/** The columns aligned right: those where more than half the non-empty cells are numbers, headings included. */
function numericColumns(rows: readonly (readonly string[])[], width: number) {
  return Array.from({ length: width }, (_column, index) => {
    let filled = 0;
    let numbers = 0;
    for (const row of rows) {
      const cell = row[index] ?? "";
      if (!cell) continue;
      filled++;
      if (NUMBER.test(cell)) numbers++;
    }
    return numbers * 2 > filled;
  });
}

/** A cell longer than this may end in an ellipsis at the column's width; its full text shows on hover. */
const LONG_CELL = 20;

/** What the panel says when the gateway can't show the sheet. */
function sheetFailure(code: string): MessageKey {
  switch (code) {
    case "preview_too_large":
      return "files.sheetTooComplex";
    case "preview_damaged":
      return "files.sheetDamaged";
    case "preview_protected":
      return "files.sheetProtected";
    case "not_found":
    case "path_invalid":
      return "files.goneFile";
    default:
      return "files.sheetFailed";
  }
}

/**
 * A spreadsheet's sheet as a read-only table: the gateway reads it (calamine, and its own
 * counting reader for .ods) and answers display text only, at most 500 rows by 50 columns. Cells are plain text, never
 * markup or links, even when they read like a URL or `=HYPERLINK(…)`. A file over the preview limit isn't fetched.
 */
export function SheetPreview({
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
  const [chosen, setChosen] = useState<string | undefined>(undefined);
  const small = size <= limit;
  const sheet = useQuery({
    queryKey: ["file-sheet", conversationId, path, etag, chosen ?? null],
    queryFn: async ({ signal }) => {
      try {
        return await fileSheet(api, conversationId, path, chosen, signal);
      } catch (error) {
        // A chosen sheet that's gone (the file changed): the first sheet instead.
        if (
          chosen === undefined ||
          !(error instanceof ApiError) ||
          error.code !== "not_found"
        )
          throw error;
        return fileSheet(api, conversationId, path, undefined, signal);
      }
    },
    enabled: small,
    retry: false,
    // Switching sheets keeps the picker and the last table until the next one arrives.
    placeholderData: keepPreviousData,
  });
  const code =
    sheet.isError && sheet.error instanceof ApiError ? sheet.error.code : "";
  const tooLarge = (bytes: number) => (
    <p className="type-small muted files-state">
      {t("files.sheetTooLarge", {
        size: formatSize(bytes, t.locale),
        limit: formatSize(limit, t.locale),
      })}
    </p>
  );
  if (!small) return tooLarge(size);
  if (sheet.isError) {
    if (code === "body_too_large") return tooLarge(size);
    return (
      <p className="type-small muted files-state">{t(sheetFailure(code))}</p>
    );
  }
  if (!sheet.data) return <Skeleton label={t("files.opening")} />;
  return (
    <div className="file-kind stack">
      {sheet.data.sheets.length > 1 && (
        <SheetPicker
          sheets={sheet.data.sheets}
          current={sheet.data.sheet}
          onChoose={setChosen}
        />
      )}
      <SheetTable name={name} sheet={sheet.data} />
    </div>
  );
}

/** The workbook's sheets: a row of buttons, or a list when there are many. */
function SheetPicker({
  sheets,
  current,
  onChoose,
}: {
  sheets: readonly string[];
  current: string;
  onChoose: (sheet: string) => void;
}) {
  const t = useT();
  if (sheets.length >= PICKER_LIST)
    return (
      // The standard picker (WEB-U-134), with its search for a long list.
      <MenuButton
        label={t("files.sheets")}
        value={current}
        options={sheets.map((sheet) => ({ id: sheet, label: sheet }))}
        onChange={onChoose}
      />
    );
  return (
    <div
      className="segmented file-sheets"
      role="group"
      aria-label={t("files.sheets")}
    >
      {sheets.map((sheet) => (
        <Button
          key={sheet}
          variant="ghost"
          aria-pressed={sheet === current}
          onClick={() => onChoose(sheet)}
        >
          {sheet}
        </Button>
      ))}
    </div>
  );
}

/**
 * Whether the table runs past its box on the right (WEB-U-182): the cut edge then fades, so columns beyond it read as
 * more to scroll to, not as missing. Measured as the box resizes and as it scrolls.
 */
function useMoreToTheRight(box: RefObject<HTMLDivElement | null>) {
  const [more, setMore] = useState(false);
  useLayoutEffect(() => {
    const element = box.current;
    if (!element) return;
    const measure = () =>
      setMore(
        element.scrollLeft + element.clientWidth < element.scrollWidth - 1,
      );
    measure();
    element.addEventListener("scroll", measure, { passive: true });
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(measure);
    observer?.observe(element);
    return () => {
      element.removeEventListener("scroll", measure);
      observer?.disconnect();
    };
  }, [box]);
  return more;
}

/** The shown rows under column letters, each row numbered, with how much of the sheet they are. */
function SheetTable({ name, sheet }: { name: string; sheet: Sheet }) {
  const t = useT();
  const box = useRef<HTMLDivElement>(null);
  const more = useMoreToTheRight(box);
  const shown = sheet.rows.length;
  if (shown === 0 && !sheet.more_rows)
    return (
      <p className="type-small muted files-state">{t("files.sheetEmpty")}</p>
    );
  const width = sheet.rows[0]?.length ?? 0;
  const number = (value: number) => formatNumber(value, t.locale);
  const rows =
    sheet.more_rows && sheet.total_rows !== null
      ? t("files.rowsShown", {
          shown: number(shown),
          count: number(sheet.total_rows),
        })
      : sheet.more_rows
        ? t("files.rowsFirst", { shown: number(shown) })
        : t("files.rows", { count: shown });
  const label = t("files.sheetTable", { sheet: sheet.sheet, name });
  const right = numericColumns(sheet.rows, width);
  return (
    <>
      <p className="type-small muted">
        {rows}
        {` · ${
          sheet.more_columns
            ? t("files.columnsFirst", { shown: number(width) })
            : t("files.columns", { count: width })
        }`}
        {sheet.truncated_cells > 0 &&
          ` · ${t("files.cellsCut", { count: sheet.truncated_cells })}`}
      </p>
      <div
        ref={box}
        className="file-table file-sheet"
        role="region"
        aria-label={label}
        tabIndex={0}
        data-more={more || undefined}
      >
        <table aria-label={label}>
          <thead>
            <tr>
              <td />
              {Array.from({ length: width }, (_cell, index) => (
                <th
                  key={index}
                  scope="col"
                  className={right[index] ? "file-cell-number" : undefined}
                >
                  {columnName(index)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {sheet.rows.map((row, rowIndex) => (
              <tr key={rowIndex}>
                <th scope="row" className="file-sheet-row">
                  {rowIndex + 1}
                </th>
                {row.map((cell, index) => (
                  <td
                    key={index}
                    className={right[index] ? "file-cell-number" : undefined}
                    title={cell.length > LONG_CELL ? cell : undefined}
                  >
                    {/* The cap sits on the cell's content: a table stretches cells past their own max-width. */}
                    <span className="file-cell">{cell}</span>
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}
