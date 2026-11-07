import type { Upload } from "../protocol/generated/api";
import { parseApi } from "../protocol/validate";

import { ApiError, failure } from "./error";
import { pathFor } from "./operations";

/** The part of XMLHttpRequest an upload uses; fetch cannot report upload progress. */
export type UploadRequest = {
  open(method: string, url: string): void;
  setRequestHeader(name: string, value: string): void;
  getResponseHeader(name: string): string | null;
  send(body: Blob): void;
  abort(): void;
  readonly status: number;
  readonly responseText: string;
  upload: { onprogress: ((event: ProgressEvent) => void) | null };
  onload: ((event: ProgressEvent) => void) | null;
  onerror: ((event: ProgressEvent) => void) | null;
  onabort: ((event: ProgressEvent) => void) | null;
};

export type UploadOptions = {
  onProgress?: (loaded: number, total: number) => void;
  signal?: AbortSignal;
  request?: () => UploadRequest;
};

/** Sends one file as the raw request body into the workspace's uploads/; the gateway picks the final name. */
export function uploadFile(
  conversationId: string,
  file: File,
  options: UploadOptions = {},
): Promise<Upload> {
  return new Promise((resolve, reject) => {
    const request = (options.request ?? (() => new XMLHttpRequest()))();
    const cancelled = () => request.abort();
    const settle = (outcome: () => void) => {
      options.signal?.removeEventListener("abort", cancelled);
      outcome();
    };
    request.open(
      "POST",
      pathFor(
        "upload",
        { id: conversationId },
        new URLSearchParams({ name: file.name }),
      ),
    );
    request.setRequestHeader("accept", "application/json");
    request.setRequestHeader("content-type", "application/octet-stream");
    request.upload.onprogress = (event) =>
      options.onProgress?.(
        event.loaded,
        event.lengthComputable ? event.total : file.size,
      );
    request.onload = () =>
      settle(() => {
        const value = readJson(request);
        if (request.status !== 201) {
          const retry = request.getResponseHeader("retry-after");
          reject(
            failure(
              request.status,
              new Headers(retry ? { "retry-after": retry } : {}),
              value,
            ),
          );
          return;
        }
        try {
          resolve(parseApi(value, "Upload"));
        } catch {
          reject(
            new ApiError(
              request.status,
              "invalid_response",
              "The server response could not be read. Please try again.", // copy-lint: not user copy
            ),
          );
        }
      });
    request.onerror = () =>
      settle(() =>
        reject(
          new ApiError(
            0,
            "network",
            "The gateway could not be reached. Please try again.", // copy-lint: not user copy
          ),
        ),
      );
    request.onabort = () =>
      settle(
        () =>
          reject(new ApiError(0, "cancelled", "The request was cancelled.")), // copy-lint: not user copy
      );
    options.signal?.addEventListener("abort", cancelled, { once: true });
    if (options.signal?.aborted) {
      cancelled();
      return;
    }
    request.send(file);
  });
}

function readJson(request: UploadRequest): unknown {
  if (
    !request
      .getResponseHeader("content-type")
      ?.toLowerCase()
      .startsWith("application/json")
  )
    return null;
  try {
    return JSON.parse(request.responseText) as unknown;
  } catch {
    return null;
  }
}
