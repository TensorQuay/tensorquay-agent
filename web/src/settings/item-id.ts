/** The most characters an ID has (the gateway's name rule for environments and profiles). */
const MAX_ID = 64;

/**
 * A new item's ID made from its display name (the UI standard's Settings pages, WEB-U-097): lower-case letters and
 * digits with "-" between words, starting with a letter, never `reserved`, and a number added when one is taken. Empty
 * when the display name has no letters to use; the page then asks for the ID itself.
 */
export function itemId(
  display: string,
  taken: ReadonlySet<string>,
  reserved?: string,
): string {
  const words = display
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^[^a-z]+/, "");
  const cut = (text: string, length: number) =>
    text.slice(0, length).replace(/-+$/, "");
  let base = cut(words, MAX_ID);
  if (!base) return "";
  if (base === reserved) base = `${reserved}-environment`;
  let id = base;
  for (let count = 2; taken.has(id); count++) {
    const suffix = `-${count}`;
    id = `${cut(base, MAX_ID - suffix.length)}${suffix}`;
  }
  return id;
}
