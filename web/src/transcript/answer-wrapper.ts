/** A paragraph opening with one of the wrapper tags some models (qwen) put around a final answer. */
const OPENER = /(^|\n\n)<(result|note)>/g;
/** A line that opens or closes a fenced code block. */
const FENCE = /^(?:```|~~~)/gm;

/**
 * A final answer as the reader should see it (C2, WEB-U-135): when its whole text, or its whole trailing paragraph,
 * is wrapped in `<result>…</result>` or `<note>…</note>`, the inner text without the tag. A tag anywhere else (inline,
 * a paragraph that isn't the last, inside a code block, unclosed or mismatched) is the model's own text and stays.
 */
export function unwrapAnswer(text: string): string {
  const whole = text.trim();
  // The last paragraph that opens with a wrapper is the only candidate: a wrapper must reach the answer's end.
  const openers = [...whole.matchAll(OPENER)];
  const last = openers.at(-1);
  if (!last) return text;
  const start = last.index + (last[1]?.length ?? 0);
  const tag = last[2] ?? "";
  const rest = whole.slice(start);
  const open = `<${tag}>`;
  const close = `</${tag}>`;
  if (!rest.endsWith(close)) return text;
  const inner = rest.slice(open.length, rest.length - close.length);
  // One pair only, with something inside it.
  if (inner.includes(close) || inner.includes(open) || !inner.trim())
    return text;
  // Inside an open code block, the tags are code.
  const fences = whole.slice(0, start).match(FENCE)?.length ?? 0;
  if (fences % 2 === 1) return text;
  return whole.slice(0, start) + inner.trim();
}
