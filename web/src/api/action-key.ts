import { ApiError } from "./error";

/** Creates a v4 UUID even on HTTP; fails explicitly when secure randomness is unavailable. */
export function newActionKey(): string {
  const source = globalThis.crypto;
  if (typeof source?.randomUUID === "function") return source.randomUUID();
  if (typeof source?.getRandomValues !== "function")
    throw new ApiError(
      0,
      "randomness_unavailable",
      "This browser cannot create a secure request key. Try another browser.", // copy-lint: not user copy
    );
  const hex = Array.from(
    source.getRandomValues(new Uint8Array(16)),
    (byte, index) => {
      const value =
        index === 6
          ? (byte & 0x0f) | 0x40
          : index === 8
            ? (byte & 0x3f) | 0x80
            : byte;
      return value.toString(16).padStart(2, "0");
    },
  ).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
