// @vitest-environment node
import { describe, expect, it } from "vitest";

import { parseStackFrame } from "../../e2e/stack-frames";
import { transcriptFrames } from "../test/frames";

describe("WEB-U-003 real-stack frame observer", () => {
  it("validates recorded transcript frames in the Node harness", () => {
    for (const frame of transcriptFrames)
      expect(parseStackFrame(JSON.stringify(frame))).toEqual(frame);
  });

  it("rejects malformed frames and invalid protocol formats", () => {
    const frame = transcriptFrames.find((value) => value.type === "event");
    for (const source of [
      "{",
      JSON.stringify({ type: "future" }),
      JSON.stringify({ ...frame, event: undefined }),
      JSON.stringify({ ...frame, ts: "not-a-date" }),
      JSON.stringify({ ...frame, seq: Number.MAX_SAFE_INTEGER + 1 }),
    ])
      expect(() => parseStackFrame(source)).toThrow();
  });

  it("bounds observed UTF-8 bytes even when the string length is smaller", () => {
    const source = JSON.stringify({
      type: "error",
      code: "busy",
      message: "例".repeat(90_000),
      retryable: true,
    });
    expect(source.length).toBeLessThan(262144);
    expect(() => parseStackFrame(source)).toThrow("Invalid event size");
  });
});
