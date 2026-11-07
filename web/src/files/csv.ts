/**
 * Rows of a CSV or TSV text (RFC 4180): quoted fields may hold the separator, doubled quotes and line breaks. Returns
 * null when it isn't a table (an unclosed quote, or no rows). A preview cut short (`truncated`) loses its last row,
 * which may be partial.
 */
export function parseTable(
  text: string,
  separator: "," | "\t",
  truncated = false,
): string[][] | null {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;
  let index = 0;
  const endRow = () => {
    row.push(field);
    rows.push(row);
    row = [];
    field = "";
  };
  while (index < text.length) {
    const char = text[index];
    if (quoted) {
      if (char === '"' && text[index + 1] === '"') {
        field += '"';
        index += 2;
        continue;
      }
      if (char === '"') quoted = false;
      else field += char;
      index += 1;
      continue;
    }
    if (char === '"' && field === "") quoted = true;
    else if (char === separator) {
      row.push(field);
      field = "";
    } else if (char === "\r" && text[index + 1] === "\n") {
      endRow();
      index += 1;
    } else if (char === "\n" || char === "\r") endRow();
    else field += char;
    index += 1;
  }
  if (quoted) {
    if (!truncated) return null;
    row = [];
    field = "";
  } else if ((field !== "" || row.length > 0) && !truncated) endRow();
  return rows.length > 0 ? rows : null;
}
