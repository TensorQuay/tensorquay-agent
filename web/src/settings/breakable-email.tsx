/**
 * An email that may break only after its @ or a dot, never inside a word (WEB-U-117, a first-hour check): read whole,
 * on one line wherever the table has room. People's table and the Sandboxes owner column use it, so a column is
 * sized by the address's parts and a long address wraps between them instead of running under the next column.
 */
export function breakable(email: string) {
  return email
    .split(/(?<=[@.])/)
    .flatMap((part, index) =>
      index === 0 ? [part] : [<wbr key={index} />, part],
    );
}
