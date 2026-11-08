import { firstTitle } from "../conversations/first-title";

/** A purpose's first sentence: up to its full stop (or 。！？) or the end of its first line. */
const firstSentence = (purpose: string) =>
  purpose.trim().split(/(?<=[.!?])\s+|(?<=[。！？])|\r?\n/, 1)[0] ?? "";

/**
 * A channel's title from what it's for (WEB-U-062): its first sentence, at most sixty characters ending at a word, with
 * no closing full stop. People see the title everywhere; they never type an identifier.
 */
export function suggestTitle(purpose: string): string {
  const sentence = firstSentence(purpose);
  return firstTitle(sentence).replace(/[.。]$/, "");
}

/**
 * What a channel's purpose says beyond its title, for the line under it (a pass over the pages before 17.0; WEB-U-250).
 * A title suggested from the purpose is its first sentence, cut at sixty characters with "…": the line then carries on
 * from the cut ("…each part.") or gives the sentences after it, never the title's words again. Null when the purpose
 * adds nothing; a purpose under a title of the person's own is shown whole.
 */
export function purposeBeyond(purpose: string, title: string): string | null {
  const text = purpose.trim();
  if (!text) return null;
  const same = (a: string, b: string) =>
    a.trim().toLowerCase() === b.trim().toLowerCase();
  if (!same(suggestTitle(text), title)) return text;
  const head = title.endsWith("…") ? title.slice(0, -1).trimEnd() : null;
  const rest = head
    ? text.slice(head.length)
    : text.slice(firstSentence(text).length);
  const left = rest.trim();
  if (!left) return null;
  return head ? `…${left}` : left;
}
