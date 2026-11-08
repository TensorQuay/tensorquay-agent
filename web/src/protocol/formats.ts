import { validIpv6 } from "./ipv6";

/**
 * The schema formats the web checks, which the generated validators call by name (scripts/validators.mjs declares the
 * same names and shapes; a test keeps them in step). A string format is a function; a number format is an object that
 * applies to numbers only, as ajv's format definitions are.
 */
export type Format =
  | ((value: string) => boolean)
  | { type: "number"; validate: (value: number) => boolean };

const unsigned = (maximum: number): Format => ({
  type: "number",
  validate: (value) =>
    Number.isSafeInteger(value) && value >= 0 && value <= maximum,
});
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const DATE_TIME =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/i;
const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
// An absolute URI (RFC 3986): a scheme, then no whitespace or control characters, and one the browser can parse (a
// connector's MCP server address).
const URI = /^[a-z][a-z0-9+.-]*:[^\s\p{Cc}]+$/iu;

export const formats: Readonly<Record<string, Format>> = {
  ipv6: validIpv6,
  uint16: unsigned(65535),
  uint32: unsigned(4294967295),
  uint64: unsigned(Number.MAX_SAFE_INTEGER),
  int32: {
    type: "number",
    validate: (value) =>
      Number.isInteger(value) && value >= -2147483648 && value <= 2147483647,
  },
  // Signed 64-bit values (Unix seconds on the workspace link), bounded to what JSON numbers carry exactly, as uint64 is.
  int64: { type: "number", validate: (value) => Number.isSafeInteger(value) },
  uuid: (value) => UUID.test(value),
  "date-time": (value) =>
    DATE_TIME.test(value) && Number.isFinite(Date.parse(value)),
  email: (value) => EMAIL.test(value),
  uri: (value) => URI.test(value) && URL.canParse(value),
  // A calendar date (the admin usage report's `from` and `to`): YYYY-MM-DD that names a real day (WEB-U-036).
  date: (value) => {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
    const day = new Date(`${value}T00:00:00Z`);
    return (
      Number.isFinite(day.getTime()) && day.toISOString().slice(0, 10) === value
    );
  },
};
