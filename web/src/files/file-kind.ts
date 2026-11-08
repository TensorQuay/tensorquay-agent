/**
 * What a file is shown as, by its extension: small on purpose, the conversation page loads it.
 */
export type PreviewKind =
  "image" | "document" | "table" | "page" | "pdf" | "sheet" | "text";

/** The image types the panel draws, by extension. */
export const IMAGE_TYPES: Readonly<Record<string, string>> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  svg: "image/svg+xml",
};

export const extension = (name: string) =>
  name.includes(".") ? name.slice(name.lastIndexOf(".") + 1).toLowerCase() : "";

export function previewKind(name: string): PreviewKind {
  const kind = extension(name);
  if (kind in IMAGE_TYPES) return "image";
  if (kind === "md" || kind === "markdown") return "document";
  if (kind === "csv" || kind === "tsv") return "table";
  if (kind === "html" || kind === "htm") return "page";
  if (kind === "pdf") return "pdf";
  // Legacy .xls is not read (the gateway answers preview_unsupported); it downloads like any other file.
  if (kind === "xlsx" || kind === "xlsm" || kind === "xlsb" || kind === "ods")
    return "sheet";
  return "text";
}

/**
 * Whether the files panel shows the file as what it is (an image, a document, a table, a web page, a PDF, a
 * spreadsheet, plain text), so a finished turn may offer to open it; a Word file it would only offer to download.
 */
export const shownInPanel = (name: string) =>
  previewKind(name) !== "text" || extension(name) === "txt";
