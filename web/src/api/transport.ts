import { ApiError, failure } from "./error";
import {
  methodFor,
  operationName,
  pathFor,
  routeOf,
  type Endpoint,
  type Operation,
} from "./operations";
import { SessionRedirect } from "./session-redirect";

export type Environment = {
  fetch: typeof globalThis.fetch;
  redirect: (path: string) => void;
  currentPath: () => string;
  sleep: (milliseconds: number, signal?: AbortSignal) => Promise<void>;
};
export type RequestOptions = {
  parameters?: Record<string, string>;
  query?: URLSearchParams;
  body?: string;
  key?: string;
  signal?: AbortSignal | undefined;
  /** Error statuses whose body is still the answer (for example readiness: 503 with the failing checks). */
  accept?: readonly number[];
  /** A successful answer's body as bytes (a file's preview); an error's is still read as JSON. */
  read?: "json" | "blob";
  /** How long to wait for an answer the gateway bounds itself for longer, such as a route check; otherwise 10 s. */
  timeoutMs?: number;
};

/** How long a request waits for its answer unless it names its own timeout. */
export const REQUEST_TIMEOUT_MS = 10_000;

export class Transport {
  private readonly sessionRedirect = new SessionRedirect();
  constructor(readonly environment: Environment) {}

  async request<T>(
    operation: Operation | Endpoint,
    decode: (value: unknown, headers: Headers) => T,
    options: RequestOptions = {},
  ): Promise<T> {
    const name = operationName(operation);
    const session = this.sessionRedirect.started(name);
    const abort = new AbortController();
    let timedOut = false;
    const timeout = setTimeout(() => {
      timedOut = true;
      abort.abort();
    }, options.timeoutMs ?? REQUEST_TIMEOUT_MS);
    const cancelled = () => {
      abort.abort();
    };
    options.signal?.addEventListener("abort", cancelled, { once: true });
    if (options.signal?.aborted) abort.abort();
    try {
      const headers = new Headers({ accept: "application/json" });
      if (options.body !== undefined)
        headers.set("content-type", "application/json");
      if (options.key !== undefined)
        headers.set("idempotency-key", options.key);
      const response = await this.environment.fetch(
        pathFor(operation, options.parameters, options.query),
        {
          method: methodFor(operation),
          credentials: "same-origin",
          redirect: "error",
          headers,
          signal: abort.signal,
          ...(options.body === undefined ? {} : { body: options.body }),
        },
      );
      if (
        response.status === 401 &&
        this.sessionRedirect.permitsExpiry(name, session)
      ) {
        const path = this.environment.currentPath();
        if (!/^\/(login|setup)(\?|$)/.test(path))
          this.environment.redirect(`/login?next=${encodeURIComponent(path)}`);
      }
      // No content (204), or accepted for later without a body (202, such as a template check), carries no value.
      const bodiless =
        response.status === 204 ||
        (response.status === 202 && !response.headers.get("content-type"));
      const value = bodiless
        ? null
        : options.read === "blob" && response.ok
          ? await response.blob()
          : await readJson(response);
      if (!response.ok && !options.accept?.includes(response.status))
        throw failure(response.status, response.headers, value);
      try {
        const result = decode(value, response.headers);
        this.sessionRedirect.succeeded(name);
        return result;
      } catch (cause) {
        // A contract failure names its route and the definition it failed, for whoever reads the error; the page words
        // it by its code.
        const why = cause instanceof Error ? cause.message : String(cause);
        throw new ApiError(
          response.status,
          "invalid_response",
          `The server response could not be read. Please try again. (${routeOf(operation)}: ${why})`, // copy-lint: not user copy
        );
      }
    } catch (error) {
      this.sessionRedirect.failed(name);
      if (error instanceof ApiError) throw error;
      if (timedOut)
        throw new ApiError(
          0,
          "timeout",
          "The request timed out. Please try again.", // copy-lint: not user copy
        );
      if (options.signal?.aborted)
        throw new ApiError(0, "cancelled", "The request was cancelled."); // copy-lint: not user copy
      throw new ApiError(
        0,
        "network",
        "The gateway could not be reached. Please try again.", // copy-lint: not user copy
      );
    } finally {
      clearTimeout(timeout);
      options.signal?.removeEventListener("abort", cancelled);
    }
  }
}

async function readJson(response: Response): Promise<unknown> {
  if (
    !response.headers
      .get("content-type")
      ?.toLowerCase()
      .startsWith("application/json")
  )
    throw new ApiError(
      response.status,
      "invalid_response",
      "The server response could not be read.", // copy-lint: not user copy
    );
  const reader = response.body?.getReader();
  if (!reader)
    throw new ApiError(
      response.status,
      "invalid_response",
      "The server response was empty.", // copy-lint: not user copy
    );
  const decoder = new TextDecoder();
  let length = 0;
  let text = "";
  try {
    while (true) {
      const part = await reader.read();
      if (part.done) break;
      length += part.value.byteLength;
      if (length > 2 * 1024 * 1024)
        throw new ApiError(
          response.status,
          "invalid_response",
          "The server response was too large.", // copy-lint: not user copy
        );
      text += decoder.decode(part.value, { stream: true });
    }
    text += decoder.decode();
    try {
      return JSON.parse(text) as unknown;
    } catch {
      throw new ApiError(
        response.status,
        "invalid_response",
        "The server response could not be read.", // copy-lint: not user copy
      );
    }
  } finally {
    await reader.cancel().catch(() => undefined);
    reader.releaseLock();
  }
}

export function delay(
  milliseconds: number,
  signal?: AbortSignal,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const cancel = () => {
      clearTimeout(timer);
      signal?.removeEventListener("abort", cancel);
      reject(new ApiError(0, "cancelled", "The request was cancelled.")); // copy-lint: not user copy
    };
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", cancel);
      resolve();
    }, milliseconds);
    signal?.addEventListener("abort", cancel, { once: true });
    if (signal?.aborted) cancel();
  });
}
