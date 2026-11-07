/**
 * The marked blocks tools write for the model (tqa-tools untrusted.rs `marked`): text from outside the conversation in
 * `<untrusted-content …>`, a signed-in person's words in `<signed-in-person …>`. The tags are the model's, never words
 * for a person.
 */

/** A block's opening line: its tag, its source and attributes (escaped, so on one line), and the id that closes it. */
export const OPENING =
  /^<(signed-in-person|untrusted-content) source="[a-z_]+"(?: [a-z_]+="[^"\n]*")* id="([0-9a-f]{16})">$/;

/** A closing line, whichever block it closes. */
const CLOSING = /^<\/(signed-in-person|untrusted-content) id="[0-9a-f]{16}">$/;

/**
 * A tool's text as a person reads it (WEB-U-363): each block's opening and closing lines dropped, the text inside and
 * around them kept. A block ends only at the closing line with its own id, so text inside that looks like another tag
 * stays as text. A block that never closes is left whole; a closing line outside any block (its opening cut off by a
 * bounded output) is dropped.
 */
export function unmarked(text: string): string {
  const lines = text.split("\n");
  const kept: string[] = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index] ?? "";
    const opening = OPENING.exec(line);
    if (opening) {
      const closing = `</${opening[1]} id="${opening[2]}">`; // copy-lint: not user copy
      const end = lines.indexOf(closing, index + 1);
      if (end !== -1) {
        kept.push(...lines.slice(index + 1, end));
        index = end + 1;
        continue;
      }
    } else if (CLOSING.test(line)) {
      index += 1;
      continue;
    }
    kept.push(line);
    index += 1;
  }
  return kept.join("\n");
}
