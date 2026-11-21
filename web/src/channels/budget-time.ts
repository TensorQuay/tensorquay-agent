/**
 * When a channel's daily budget renews, as its viewer reads it (decided on 21 Nov 2026, C-14): their own weekday and
 * time, enough within the day the window lasts ("Sun 11:00 AM").
 */
export function resetTime(at: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    weekday: "short",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(at));
}
