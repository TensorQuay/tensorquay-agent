"use client";

import { useRef, type ClipboardEvent } from "react";

/**
 * The stem of a pasted image's name: a workspace file name, which the gateway keeps to ASCII and the agent reads, so
 * it is the same in every language, as every other upload's name is.
 */
const PASTED_IMAGE = "pasted-image"; // copy-lint: not user copy (a workspace file name)
/** The extension a pasted image gets, by the type the clipboard declares. */
const EXTENSIONS: Readonly<Record<string, string>> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/gif": "gif",
  "image/webp": "webp",
  "image/bmp": "bmp",
};
/** A browser's placeholder name for an image that has none (a screenshot on the clipboard). */
const GENERIC = /^(?:image|unknown)?(?:\.[a-z0-9]+)?$/i;

/** The files a paste carries, in clipboard order; none for text. */
export function pastedFiles(data: DataTransfer | null): File[] {
  if (!data) return [];
  const items = Array.from(data.items ?? []).flatMap((item) => {
    if (item.kind !== "file") return [];
    const file = item.getAsFile();
    return file ? [file] : [];
  });
  return items.length > 0 ? items : Array.from(data.files ?? []);
}

/** A pasted image with no name of its own is "pasted-image-N.<ext>" (a name the gateway keeps as it is); any other file keeps its name. */
export function namePasted(file: File, next: () => number): File {
  const extension = EXTENSIONS[file.type];
  if (!extension || !GENERIC.test(file.name)) return file;
  return new File([file], `${PASTED_IMAGE}-${next()}.${extension}`, {
    type: file.type,
    lastModified: file.lastModified,
  });
}

/**
 * Paste into one element: a clipboard holding files (a screenshot, a copied file) attaches them and nothing else
 * happens; one holding text is left to the browser. `onFiles` null offers nothing, as useDrop.
 */
export function usePaste(onFiles: ((files: File[]) => void) | null) {
  const count = useRef(0);
  if (!onFiles) return {};
  return {
    onPaste: (event: ClipboardEvent) => {
      const files = pastedFiles(event.clipboardData);
      if (files.length === 0) return;
      event.preventDefault();
      onFiles(files.map((file) => namePasted(file, () => ++count.current)));
    },
  };
}
