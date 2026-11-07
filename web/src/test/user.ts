import type { User } from "../protocol/generated/api";

/**
 * The gateway's default file limits ([files]: uploads of 25 MiB a file and 1 GiB a workspace; previews of 2 MiB an
 * HTML page, 20 MiB a PDF and 10 MiB a spreadsheet), as sign-in and me carry them.
 */
export const LIMITS: User["limits"] = {
  upload_bytes: 25 * 1024 * 1024,
  uploads_bytes: 1024 * 1024 * 1024,
  preview_html_bytes: 2 * 1024 * 1024,
  preview_pdf_bytes: 20 * 1024 * 1024,
  preview_sheet_bytes: 10 * 1024 * 1024,
};
