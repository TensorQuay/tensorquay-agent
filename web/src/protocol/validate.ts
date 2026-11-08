import { formats } from "./formats";
import type { Schemas } from "./generated/api";
import type { ServerFrame } from "./generated/protocol";
import {
  api,
  eventKinds,
  frameKinds,
  problemKinds,
  ServerFrame as validFrame,
} from "./generated/validators.core";

/**
 * The page's validators are precompiled from the generated schemas (scripts/validators.mjs), so the page ships no
 * schema compiler and runs no generated code at run time (a Content-Security-Policy without 'unsafe-eval' holds).
 * The core module every page loads checks the stream's frames and the API answers decoded outside an area; an area's
 * module (channels, settings, admin) registers its own when a file that decodes them imports it.
 */
type Validate = (value: unknown) => boolean;
const API_SCHEMA = "urn:tqa:api";
const registry = new Map<string, Map<string, Validate>>([
  [API_SCHEMA, new Map(Object.entries(api))],
]);

/** Adds an area's validators for schema `id` (called by the generated area modules). */
export function registerValidators(
  id: string,
  validators: Readonly<Record<string, Validate>>,
) {
  const known = registry.get(id) ?? new Map<string, Validate>();
  for (const [name, validate] of Object.entries(validators))
    known.set(name, validate);
  registry.set(id, known);
}

/** Whether the validators know a schema format; an unknown one would refuse every answer that uses it. */
export function knownFormat(name: string): boolean {
  return name in formats;
}

/** A frame or event kind this web does not know: kept only for its sequence, rendered as nothing. */
export type IgnoredFrame = { type: "ignored"; seq?: number };
export type ParsedFrame = ServerFrame | IgnoredFrame;

const FRAME_KINDS: ReadonlySet<string> = new Set(frameKinds);
const EVENT_KINDS: ReadonlySet<string> = new Set(eventKinds);
const PROBLEM_KINDS: ReadonlySet<string> = new Set(problemKinds);

const record = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);
const validSeq = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;

/**
 * ARCHITECTURE §15: a frame kind, or an event kind inside a well-formed event frame, that this web does not know is
 * accepted as an opaque marker so the stream keeps its order; anything of a known kind that fails validation is
 * still an error.
 */
function unknownKind(value: unknown): IgnoredFrame | null {
  if (!record(value) || typeof value.type !== "string") return null;
  if (!FRAME_KINDS.has(value.type)) {
    if (!("seq" in value)) return { type: "ignored" };
    return validSeq(value.seq) ? { type: "ignored", seq: value.seq } : null;
  }
  if (value.type !== "event") return null;
  const event = value.event;
  if (!record(event) || typeof event.type !== "string") return null;
  if (EVENT_KINDS.has(event.type)) return null;
  if (!validSeq(value.seq) || typeof value.ts !== "string") return null;
  return { type: "ignored", seq: value.seq };
}

/** A failure code as the protocol writes them: a bare token. */
const CODE_TOKEN = /^[a-z0-9_]{1,64}$/;

/**
 * A known frame whose only fault is a failure code newer than this web (TOOL-U-036's tolerant decode): an event's or
 * an error frame's `code` that the generated enum doesn't list. It is checked with a code the enum does list in its
 * place, so every other field is still validated, and kept with its own code, which the page words as "The run
 * stopped" with the code after it. Without this a newer runtime's run.failed was dropped and the page never left
 * "Working…".
 */
function newerCode(value: unknown): boolean {
  if (!record(value)) return false;
  const holder =
    value.type === "event" && record(value.event) ? value.event : value;
  if (typeof holder.code !== "string" || !CODE_TOKEN.test(holder.code))
    return false;
  const known = structuredClone(value);
  const target =
    known.type === "event" && record(known.event) ? known.event : known;
  target.code = "internal";
  return validFrame(known);
}

/**
 * A retry whose only fault is a problem kind newer than this web (WEB-U-417; the runtime reads an unknown kind as
 * `other`, PROTO-U-044): checked, and kept, with the problem as `other`, so every other field is still validated. Without
 * this a tab still running the old app after an upgrade threw "Invalid protocol event" on the retry.
 */
function newerProblem(value: unknown): ServerFrame | null {
  if (!record(value) || value.type !== "event" || !record(value.event))
    return null;
  const problem = value.event.fallback_problem;
  if (
    value.event.type !== "provider.retry" ||
    !record(problem) ||
    typeof problem.kind !== "string" ||
    !CODE_TOKEN.test(problem.kind) ||
    PROBLEM_KINDS.has(problem.kind)
  )
    return null;
  const known = structuredClone(value);
  (known.event as Record<string, unknown>).fallback_problem = { kind: "other" };
  return validFrame(known) ? (known as ServerFrame) : null;
}

/** Validates untrusted stream input before it reaches the reducer or a component. */
export function parseFrame(source: string): ParsedFrame {
  if (
    source.length > 262144 ||
    new TextEncoder().encode(source).length > 262144
  )
    // copy-lint: not user copy (internal diagnostic)
    throw new Error("Invalid event size");
  const value: unknown = JSON.parse(source);
  if (validFrame(value) || newerCode(value)) return value as ServerFrame;
  const newer = newerProblem(value);
  if (newer) return newer;
  const ignored = unknownKind(value);
  // copy-lint: not user copy (internal diagnostic)
  if (!ignored) throw new Error("Invalid protocol event");
  return ignored;
}

/** Validates an API answer against one definition of the registered schema `id`. */
export function parseApiIn<Name extends keyof Schemas>(
  id: string,
  value: unknown,
  name: Name,
): Schemas[Name] {
  const validate = registry.get(id)?.get(name);
  if (!validate)
    // copy-lint: not user copy (internal diagnostic): a decoding file must import its area's validators
    throw new Error(`No validator for ${id} ${name}`);
  if (!validate(value))
    // copy-lint: not user copy (internal diagnostic)
    throw new Error(`Invalid API response: ${name} (${id})`);
  return value as Schemas[Name];
}

export function parseApi<Name extends keyof Schemas>(
  value: unknown,
  name: Name,
): Schemas[Name] {
  return parseApiIn(API_SCHEMA, value, name);
}
