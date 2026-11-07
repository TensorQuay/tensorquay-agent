import type { MessageValues } from "../i18n/message";
import { english, type MessageKey, type Translate } from "../i18n/translate";

import { failureSentence } from "./action-failure";
import type { Failure } from "./failure-keys";

/** What a transcript notice says: a catalogue message, or text the runtime supplied (never translated). */
export type Copy =
  | { key: MessageKey; values?: MessageValues }
  | { raw: string }
  | { failure: Failure };

/** A catalogue message for a notice. */
export function message(key: MessageKey, values?: MessageValues): Copy {
  return values ? { key, values } : { key };
}

/** The notice's words in the reader's language. */
export function noticeText(copy: Copy, t: Translate = english): string {
  if ("raw" in copy) return copy.raw;
  if ("failure" in copy) return failureSentence(copy.failure, t);
  return t(copy.key, copy.values);
}
