/** The longest slug `slugify` returns. */
export const MAX_LENGTH = 40;

/**
 * Turns a title into a URL slug: lowercase ASCII letters and digits, with every run of other characters replaced by
 * one hyphen, no hyphen at either end, and at most MAX_LENGTH characters, cut so that it does not end in a hyphen.
 */
export function slugify(title: string): string {
  const slug = title
    .toLowerCase()
    .replace(/[^a-z0-9]/g, "-")
    .replace(/^-+|-+$/g, "");
  return slug.slice(0, MAX_LENGTH).replace(/-+$/, "");
}
