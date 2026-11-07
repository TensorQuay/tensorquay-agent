/** Validate a bare IPv6 literal without accepting brackets, scopes or URL delimiters. */
export function validIpv6(value: string): boolean {
  if (value.length > 45 || !/^[0-9a-f:.]+$/i.test(value)) return false;
  try {
    return new URL(`http://[${value}]/`).hostname.startsWith("[");
  } catch {
    return false;
  }
}
