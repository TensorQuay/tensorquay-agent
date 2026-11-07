/** Up to two letters for an avatar: the first letters of the email's first two name parts, or its first two characters. */
export function initials(email: string): string {
  const local = email.split("@")[0] ?? "";
  const parts = local.split(/[._+-]+/).filter(Boolean);
  const letters =
    parts.length > 1
      ? `${parts[0]?.[0] ?? ""}${parts[1]?.[0] ?? ""}`
      : local.slice(0, 2);
  return letters.toUpperCase() || "?";
}
