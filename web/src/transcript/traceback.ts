/** A Python traceback's first line, with or without the shell tool's stream mark ("[stderr] "). */
const START = /^(\[std(?:out|err)\] )?Traceback \(most recent call last\):\s*$/;
/** A frame or source line inside it: indented after the mark. */
const INSIDE = /^(\[std(?:out|err)\] )?\s/;

/**
 * A step's output with each Python traceback folded to its last line, the error itself (WEB-U-400; real-world use
 * before 17.0: ~20 lines of `File "/usr/lib/python3.13/urllib/request.py"` before "HTTP Error 429: Too Many Requests").
 * Whatever is around a trace stays; `folded` says whether anything was left out.
 */
export function foldTraceback(text: string): { text: string; folded: boolean } {
  const lines = text.split("\n");
  const kept: string[] = [];
  let folded = false;
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index] ?? "";
    if (!START.test(line)) {
      kept.push(line);
      continue;
    }
    let end = index + 1;
    while (end < lines.length && INSIDE.test(lines[end] ?? "")) end++;
    // A trace cut before its error keeps its lines: there is no last line to show instead.
    if (end >= lines.length) {
      kept.push(line);
      continue;
    }
    folded = true;
    index = end - 1;
  }
  return { text: kept.join("\n"), folded };
}
