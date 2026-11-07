import { ApiError } from "../api/error";

/** Copies code on secure and plain HTTP origins; reports failure without changing the reader's focus. */
export async function copyText(text: string): Promise<void> {
  if (typeof navigator.clipboard?.writeText === "function") {
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        navigator.clipboard.writeText(text),
        new Promise<never>((_resolve, reject) => {
          // copy-lint: not user copy (internal diagnostic)
          timer = setTimeout(() => reject(new Error("Copy timed out")), 5000);
        }),
      ]);
      return;
    } catch {
      /* Permission denial and unavailable clipboard both use the local copy fallback. */
    } finally {
      clearTimeout(timer);
    }
  }
  const previous = document.activeElement;
  const selection = document.getSelection();
  const range = selection?.rangeCount
    ? selection.getRangeAt(0).cloneRange()
    : null;
  const buffer = document.createElement("textarea");
  buffer.className = "clipboard-buffer";
  buffer.value = text;
  buffer.readOnly = true;
  buffer.tabIndex = -1;
  buffer.setAttribute("aria-hidden", "true");
  document.body.append(buffer);
  try {
    buffer.select();
    // copy-lint: not user copy (internal diagnostic)
    if (!document.execCommand("copy")) throw new Error("Copy was refused");
  } catch {
    throw new ApiError(
      0,
      "copy_failed",
      // copy-lint: not user copy (internal diagnostic)
      "Copy failed. Select the code and copy it manually.",
    );
  } finally {
    buffer.remove();
    if (previous instanceof HTMLElement)
      previous.focus({ preventScroll: true });
    if (selection && range) {
      selection.removeAllRanges();
      selection.addRange(range);
    }
  }
}
