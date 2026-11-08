import { describe, expect, it } from "vitest";

import { english } from "../i18n/translate";

import { failureCode, failureNext, failureReason } from "./failure-copy";
import { retryShort, retryWords } from "./retry-copy";

// WEB-U-364 (a first-hour check before 17.0): a model that answers with nothing (the runtime's `empty_response`,
// CORE-U) was said as "The model provider refused the request". Nothing refused it: the run and its retries say the
// model answered with nothing, and that sending again usually helps. A real refusal keeps its words.
describe("WEB-U-364 an empty model reply says so", () => {
  it("words a run ended by empty replies, on a native route and on a text route", () => {
    for (const reason of [
      "empty_response",
      "empty_response: the server returned no text on a text-protocol route; with thinking on, check its parser",
    ]) {
      const run = { code: "provider_error", reason };
      expect(failureReason(run)).toBe("The model answered with nothing");
      expect(failureNext(run)).toBe(
        "Send the message again: an empty answer is often a one-off. If it keeps happening, an administrator can check the model route.",
      );
      expect(failureReason(run)).not.toMatch(/refused/);
      // The runtime's own words stay as the code line, after the plain sentence.
      expect(failureCode(run)).toBe(reason);
    }
  });

  it("words its retries the same way, in the line and in the step's summary", () => {
    expect(english(retryWords("empty_response"))).toBe(
      "The model answered with nothing",
    );
    expect(english(retryShort("empty_response"))).toBe(
      "the model answered with nothing",
    );
  });

  it("keeps a real refusal's words", () => {
    const run = { code: "provider_error", reason: "provider_error 403" };
    expect(failureReason(run)).toBe("The model provider refused the request");
    expect(english(retryWords("http 503"))).toBe(
      "The model server answered with an error",
    );
  });
});
