/** Derives a first-line title, at most sixty Unicode code points and ending at a word when possible. */
export function firstTitle(text: string): string {
  const line = text.trim().split(/\r?\n/, 1)[0]?.trim() ?? "";
  const points = Array.from(line);
  if (points.length <= 60) return line;
  const prefix = points.slice(0, 59).join("");
  if (/\s/.test(points[59] ?? "")) return `${prefix.trimEnd()}…`;
  const boundary = prefix.search(/\s+\S*$/);
  return `${(boundary > 0 ? prefix.slice(0, boundary) : prefix).trimEnd()}…`;
}
