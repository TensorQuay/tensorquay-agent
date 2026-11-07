import { OPENING } from "./marker";

/**
 * An agent team's reads, the lead's read_reports and a helper's wait_for_lead (ADR-0034): their answer is written for
 * the model, each message one marked block (tqa-tools team/tools.rs `render`, untrusted.rs `marked`), never words for
 * a person. The page says how many messages were read; each one is in the team's log in words (UI-GUIDELINES §4: say
 * it plainly, once).
 */
export const TEAM_READS = new Set(["read_reports", "wait_for_lead"]);

/** A team read's answer when nothing waited: the runtime's own fixed words (tqa-tools team/tools.rs, `render`). */
export const NOTHING_READ = new Set([
  "Nothing new yet.", // copy-lint: not user copy
  "Nothing yet; end your turn, and you'll be woken by your lead's next message.", // copy-lint: not user copy
]);

/**
 * How many messages a team read's answer holds: its blocks, in order. A block ends only at the closing tag with its own
 * id, so a message whose text looks like another block's opening is still one message.
 */
export function teamReadCount(text: string): number {
  let count = 0;
  let closing: string | null = null;
  for (const line of text.split("\n")) {
    if (closing !== null) {
      if (line === closing) closing = null;
      continue;
    }
    const opening = OPENING.exec(line);
    if (opening) {
      count += 1;
      closing = `</${opening[1]} id="${opening[2]}">`; // copy-lint: not user copy
    }
  }
  return count;
}
