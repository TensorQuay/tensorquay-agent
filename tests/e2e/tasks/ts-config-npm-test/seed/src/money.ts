/** Amounts are whole cents, so arithmetic never rounds. */
export function addCents(...amounts: number[]): number {
  return amounts.reduce((total, amount) => total + amount, 0);
}

/** Formats cents as a currency string with two decimals, for example 1234 -> "12.34". */
export function formatCents(cents: number): string {
  const sign = cents < 0 ? "-" : "";
  const absolute = Math.abs(cents);
  return `${sign}${Math.floor(absolute / 100)}.${String(absolute % 100).padStart(2, "0")}`;
}
