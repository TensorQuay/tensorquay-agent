// @vitest-environment node
import { afterEach, describe, expect, it, vi } from "vitest";

import { newActionKey } from "./action-key";
import { ApiError } from "./error";

const uuidV4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
afterEach(() => vi.unstubAllGlobals());

describe("WEB-U-001 action keys outside a secure context", () => {
  it("uses the native UUID generator when available", () => {
    const expected = "00000000-0000-4000-8000-000000000001";
    const randomUUID = vi.fn(() => expected);
    const getRandomValues = vi.fn();
    vi.stubGlobal("crypto", { randomUUID, getRandomValues });
    expect(newActionKey()).toBe(expected);
    expect(randomUUID).toHaveBeenCalledOnce();
    expect(getRandomValues).not.toHaveBeenCalled();
  });

  it("generates distinct valid v4 UUIDs with randomUUID absent", () => {
    vi.stubGlobal("crypto", {
      getRandomValues: crypto.getRandomValues.bind(crypto),
    });
    expect("randomUUID" in crypto).toBe(false);
    const keys = Array.from({ length: 32 }, () => newActionKey());
    expect(new Set(keys).size).toBe(keys.length);
    for (const key of keys) expect(key).toMatch(uuidV4);
  });

  it.each([
    [0x00, "00000000-0000-4000-8000-000000000000"],
    [0xff, "ffffffff-ffff-4fff-bfff-ffffffffffff"],
  ])("sets version and variant bits for byte %i", (byte, expected) => {
    const getRandomValues = vi.fn((array: Uint8Array) => array.fill(byte));
    vi.stubGlobal("crypto", { getRandomValues });
    expect(newActionKey()).toBe(expected);
    expect(getRandomValues).toHaveBeenCalledOnce();
    expect(getRandomValues.mock.calls[0]?.[0]).toHaveLength(16);
  });

  it.each([undefined, {}])(
    "fails with a typed error without secure randomness: %j",
    (crypto) => {
      vi.stubGlobal("crypto", crypto);
      expect(newActionKey).toThrow(ApiError);
      expect(newActionKey).toThrow(
        expect.objectContaining({ code: "randomness_unavailable" }),
      );
    },
  );
});
