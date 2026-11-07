/** Calendar helpers on ISO dates (YYYY-MM-DD), computed in UTC. */

const DAY_MS = 24 * 60 * 60 * 1000;

export function parseDay(text: string): number {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
  if (!match) {
    throw new Error(`not an ISO date: ${text}`);
  }
  return Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
}

/** The number of days in the inclusive range from `start` to `end`; 0 when `end` is before `start`. */
export function daysInRange(start: string, end: string): number {
  const span = (parseDay(end) - parseDay(start)) / DAY_MS;
  return span < 0 ? 0 : span;
}

/** Every day of the inclusive range, as ISO dates. */
export function listDays(start: string, end: string): string[] {
  const days: string[] = [];
  const first = parseDay(start);
  for (let offset = 0; offset < daysInRange(start, end); offset++) {
    days.push(new Date(first + offset * DAY_MS).toISOString().slice(0, 10));
  }
  return days;
}
