import core from "./generated/en.core.json";
import type en from "./locales/en.json";
import { formatMessage, type MessageValues } from "./message";

/** Every key the product uses; English is the complete reference catalogue. */
export type MessageKey = keyof typeof en.messages;

/** A validated catalogue file: the language's own name and its (possibly incomplete) messages. */
export type Catalogue = {
  language: string;
  messages: Readonly<Record<string, string>>;
};

/** Translates a key for one language; `locale` feeds `Intl` for numbers, dates and plurals. */
export type Translate = ((
  key: MessageKey,
  values?: MessageValues,
) => string) & {
  readonly locale: string;
};

/**
 * The English every page can fall back to: the core catalogue, which every page loads, plus the settings pages'
 * catalogue once they have loaded it (WEB-U-014).
 */
const reference: Record<string, string> = { ...core.messages };

const areas = new Set<string>();

/** Adds one area's English messages to the reference, once its pages have loaded them. */
export function addReference(
  area: string,
  messages: Readonly<Record<string, string>>,
) {
  Object.assign(reference, messages);
  areas.add(area);
}

/** Whether an area's English is already part of the reference. */
export function hasReference(area: string): boolean {
  return areas.has(area);
}

/**
 * The copy a run's failure shows (WEB-U-392): its reasons and next steps, retries, quotas and the server's refusals. It
 * loads on its first use rather than with every page (scripts/i18n-areas.mjs's "failures" area). The refusals the
 * browser makes itself (no connection, a timeout, an answer it couldn't read) stay core, as do request errors: they
 * show when a download may not work.
 */
export const FAILURE_KEYS =
  /^(fail|retry|retryShort|quota)\.|^refuse\.(?!(network|timeout|checkArrivedNext|unknown|unknownNext)$)/;

let failures: Promise<void> | null = null;
const arrivals = new Set<() => void>();

/** Calls back once the failure copy has arrived, so a page shown without it renders again; returns the unsubscribe. */
export function onReference(listener: () => void): () => void {
  arrivals.add(listener);
  return () => arrivals.delete(listener);
}

/** Starts the failure copy's one download in the browser; the server renders it empty, as hydration's first render does. */
function loadFailures() {
  if (typeof window === "undefined") return;
  failures ??= import("./generated/en.failures.json").then(
    (loaded) => {
      addReference("failures", loaded.default.messages);
      for (const listener of arrivals) listener();
    },
    () => {
      // Tried again the next time a failure renders.
      failures = null;
    },
  );
}

/** Parses a catalogue file at the boundary; non-string messages are dropped, a missing name is an error. */
export function catalogue(value: unknown): Catalogue {
  if (typeof value !== "object" || value === null)
    throw new Error("A catalogue must be an object"); // copy-lint: not user copy
  const { language, messages } = value as {
    language?: unknown;
    messages?: unknown;
  };
  if (typeof language !== "string" || typeof messages !== "object" || !messages)
    throw new Error("A catalogue needs a language name and messages"); // copy-lint: not user copy
  return {
    language,
    messages: Object.fromEntries(
      Object.entries(messages).filter(
        (entry): entry is [string, string] => typeof entry[1] === "string",
      ),
    ),
  };
}

/** Builds a translator; a missing or malformed translation falls back to English, never to the key. */
export function translator(
  locale: string,
  source: Catalogue | null,
): Translate {
  const translate = (key: MessageKey, values: MessageValues = {}) => {
    const pattern = source?.messages[key];
    if (pattern !== undefined)
      try {
        return formatMessage(pattern, values, locale);
      } catch {
        /* A broken translation shows the English message instead. */
      }
    const fallback = reference[key];
    if (fallback === undefined && FAILURE_KEYS.test(key)) {
      // Nothing until the failure copy arrives (a moment), never the key; the page then renders again.
      loadFailures();
      return "";
    }
    return formatMessage(fallback ?? key, values, "en");
  };
  return Object.assign(translate, { locale });
}

/** The reference translator, used before a language is chosen and by pure functions in tests. */
export const english = translator("en", null);

function script(tag: string): { language: string; script: string } {
  const locale = new Intl.Locale(tag).maximize();
  return { language: locale.language, script: locale.script ?? "" };
}

/** Picks the first preferred language that an available catalogue serves (same language and script), else English. */
export function negotiate(
  preferred: readonly string[],
  available: readonly string[],
): string {
  for (const tag of preferred) {
    let wanted;
    try {
      wanted = script(tag);
    } catch {
      continue;
    }
    const match = available.find((code) => {
      const offered = script(code);
      return (
        offered.language === wanted.language && offered.script === wanted.script
      );
    });
    if (match) return match;
  }
  return "en";
}
