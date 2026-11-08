import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { ApiError } from "../api/error";
import { english } from "../i18n/translate";
import protocolSchema from "../protocol/generated/protocol.schema.json";
import { ready } from "../test/frames";

import {
  actionFailure,
  failureSentence,
  notSentMessage,
  requestFailureMessage,
} from "./action-failure";
import { knownFailure } from "./failure-keys";
import { noticeText } from "./notice-copy";
import { apply, emptyTranscript } from "./reducer";

/** Every error code the protocol defines: a stream's error frame can carry any of them. */
const codes = protocolSchema.definitions.ErrorCode.oneOf.map(
  (entry) => entry.const,
);
/** The send route's refusals a person can meet (openapi's POST …/messages), and the request's own failures. */
const refusals = [
  "conversation_deleted",
  "not_found",
  "workspace_not_ready",
  "workspace_busy",
  "body_too_large",
  "unauthorized",
  "session_required",
  "forbidden",
  "idempotency_key_reused",
  "invalid_request",
  "attachment_missing",
  "template_unavailable",
  "profile_unusable",
  "network",
  "timeout",
];
/** The bare words this test retires. */
const BARE = "something went wrong";
const UNKNOWN =
  "The gateway refused this for a reason this page doesn't recognise. Reload the page and retry; if it happens again, ask an administrator.";

// WEB-U-236 (a pass over the pages before 17.0): "something went wrong" never shows alone. Every code a person can
// meet reads as what happened and what to do, in the words the transcript uses for the same failure, and a code this
// page doesn't know still says what to do.
describe("WEB-U-236 every error a person can meet in its own words", () => {
  it.each(codes)(
    "words the stream's %s as the transcript does, never as an unknown",
    (code) => {
      const failure = actionFailure(code);
      expect(failure).toEqual(knownFailure(code, ""));
      const sentence = failureSentence(failure, english);
      expect(sentence).not.toBe(UNKNOWN);
      expect(sentence).not.toContain(BARE);
      if (/_/.test(code)) expect(sentence).not.toContain(code);
    },
  );

  it.each(refusals)(
    "words the send route's %s, never as an unknown",
    (code) => {
      const sentence = requestFailureMessage(
        new ApiError(409, code, "Private diagnostic"),
        english,
      );
      expect([UNKNOWN, BARE]).not.toContain(sentence);
      expect(sentence).not.toContain("Private diagnostic");
      expect(sentence).not.toContain(code);
    },
  );

  it("says what to do about a code this page doesn't know", () => {
    expect(
      failureSentence(actionFailure("a_code_from_a_newer_gateway"), english),
    ).toBe(UNKNOWN);
    expect(
      requestFailureMessage(
        new ApiError(409, "a_code_from_a_newer_gateway", "Private diagnostic"),
        english,
      ),
    ).toBe(UNKNOWN);
  });

  it("shows the same words as the banner and after Not sent", () => {
    const state = apply(apply(emptyTranscript(), ready), {
      type: "error",
      code: "io",
      message: "Private diagnostic",
      retryable: true,
    });
    const sentence = failureSentence(
      knownFailure("io", "") ?? actionFailure(""),
      english,
    );
    expect(state.banner && noticeText(state.banner)).toBe(sentence);
    expect(notSentMessage(sentence, english)).toBe(`Not sent. ${sentence}`);
  });

  it("shows nothing for a sequence gap, which the store mends by itself", () => {
    const state = apply(apply(emptyTranscript(), ready), {
      type: "error",
      code: "seq_gap",
      message: "",
      retryable: true,
    });
    expect(state.banner).toBeNull();
  });

  it("keeps no bare 'something went wrong' in any catalogue", () => {
    const directory = join(process.cwd(), "src", "i18n", "locales");
    for (const name of readdirSync(directory)) {
      const catalogue = JSON.parse(
        readFileSync(join(directory, name), "utf8"),
      ) as { messages: Record<string, string> };
      expect(Object.keys(catalogue.messages), name).not.toContain(
        "failure.generic",
      );
    }
  });
});
