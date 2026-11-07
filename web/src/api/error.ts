import type { Error as ApiErrorBody } from "../protocol/generated/api";
import { parseApi } from "../protocol/validate";

/** One named problem in a `400` body: the field and a reason code from a closed list (ADR-0028). */
export type FieldProblem = { field: string; reason: string };
/** What still uses a template, on `409 template_in_use` (ADR-0032). */
export type InUse = NonNullable<ApiErrorBody["error"]["in_use"]>;

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    readonly retryAfter?: number,
    readonly details: readonly FieldProblem[] = [],
    readonly inUse?: InUse,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

export function failure(
  status: number,
  headers: Headers,
  body: unknown,
): ApiError {
  let details: FieldProblem[] = [];
  let inUse: InUse | undefined;
  let code = "invalid_response";
  let message = "The server response could not be read. Please try again."; // copy-lint: not user copy
  try {
    const response = parseApi(body, "Error");
    code = response.error.code;
    message = response.error.message;
    // Field problems are a list; an agent team's runtime routes answer an object, which no browser route meets.
    details = Array.isArray(response.error.details)
      ? response.error.details
      : [];
    inUse = response.error.in_use;
  } catch {
    /* Invalid error bodies use the bounded local message. */
  }
  const retry = headers.get("retry-after");
  const seconds =
    retry == null
      ? undefined
      : /^\d+$/.test(retry)
        ? Number(retry)
        : Math.ceil((Date.parse(retry) - Date.now()) / 1000);
  return new ApiError(
    status,
    code,
    message,
    seconds !== undefined && Number.isFinite(seconds)
      ? Math.max(0, seconds)
      : undefined,
    details,
    inUse,
  );
}
