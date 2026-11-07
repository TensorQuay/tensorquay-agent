import { ApiError } from "../api/error";
import type { MessageKey } from "../i18n/translate";

/** What the panel says when the gateway refuses; `gone` names the missing folder or file. */
export function filesFailure(
  error: unknown,
  gone: MessageKey,
): { key: MessageKey; gone: boolean } {
  const code = error instanceof ApiError ? error.code : "";
  if (code === "files_unavailable")
    return { key: "files.unavailable", gone: false };
  if (code === "not_found" || code === "path_invalid")
    return { key: gone, gone: true };
  return { key: "files.failed", gone: false };
}
