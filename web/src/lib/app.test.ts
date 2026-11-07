import { describe, expect, it } from "vitest";

import { APP_NAME } from "./app";

describe("application identity", () => {
  it("uses the public application name", () => {
    expect(APP_NAME).toBe("TensorQuay Agent");
  });
});
