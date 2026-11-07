import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import en from "../i18n/locales/en.json";
import { english } from "../i18n/translate";
import protocolSchema from "../protocol/generated/protocol.schema.json";
import { frame, runId } from "../test/frames";

import {
  failureCode,
  failureNext,
  failureProvider,
  failureReason,
  knownFailure,
} from "./failure-copy";
import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

/** Every failure code the protocol defines, and those announced but not in this web's schema yet. */
const PENDING = ["reply_cut"];
const codes = [
  ...new Set([
    ...protocolSchema.definitions.ErrorCode.oneOf.map((entry) => entry.const),
    ...PENDING,
  ]),
];

/** Every budget reason the runtime writes, read from its source, so a new one without words fails here. */
function budgetReasons(): string[] {
  const found = new Set<string>();
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (name.endsWith(".rs") && !/tests?\.rs$/.test(name))
        for (const match of readFileSync(path, "utf8").matchAll(
          /ErrorCode::Budget,\s*"([^"]+)"/g,
        ))
          if (match[1]) found.add(match[1]);
    }
  };
  for (const crate of ["tqa-core", "tqa-runtime"])
    walk(join(process.cwd(), "..", "crates", crate, "src"));
  return [...found].sort();
}
const words = (key: string) => (en.messages as Record<string, string>)[key];

// The owner's test on a Mac: a failed run showed the raw "budget.output_tokens." in red. Every reason a run can fail
// with reads as what happened and what to do, in plain words; an unknown one never shows raw.
describe("WEB-U-016 every failure in plain words", () => {
  it.each(codes)("has plain words and a next step for %s", (code) => {
    const known = knownFailure(code, "diagnostic detail");
    expect(known, code).not.toBeNull();
    expect(words(known?.reason ?? "")).toBeTruthy();
    expect(words(known?.next ?? "")).toBeTruthy();
    const reason = failureReason({ code, reason: "diagnostic detail" });
    // The plain words, never the diagnostic or a raw code ("busy" may be an English word; "budget.output_tokens" isn't).
    expect(reason).toBe(words(known?.reason ?? ""));
    expect(reason).not.toContain("diagnostic detail");
    if (/[_.]/.test(code)) expect(reason).not.toContain(code);
  });

  it("finds the runtime's budget reasons in its source", () => {
    expect(budgetReasons()).toEqual(
      expect.arrayContaining(["budget.output_tokens", "budget.wall_clock"]),
    );
  });

  it.each(budgetReasons())("words the budget reason %s", (reason) => {
    const known = knownFailure("budget", reason);
    expect(known?.reason).not.toBe("run.budget");
    expect(failureReason({ code: "budget", reason })).not.toContain(reason);
  });

  it("reads the owner's failure in plain words, with what to do", () => {
    const run = { code: "budget", reason: "budget.output_tokens" };
    expect(failureReason(run)).toBe(
      "The reply was cut off at the task's length limit",
    );
    expect(failureNext(run)).toBe(
      "Ask for a shorter answer, or send a message to continue.",
    );
    expect(failureCode(run)).toBeNull();
  });

  // A model server refusing too long an input (llama.cpp's and vLLM's words): its words lead, the next step helps.
  it.each([
    "the request exceeds the available context size, try increasing it",
    "This model's maximum context length is 8192 tokens. However, you requested 9000 tokens.",
    "prompt is too long: 210000 tokens > 200000 maximum",
  ])(
    "says what helps when the server refuses too long an input: %s",
    (said) => {
      const run = {
        code: "provider_error",
        reason: "provider_error 400",
        providerMessage: said,
      };
      expect(failureReason(run)).toBe(
        "The conversation is too long for the model",
      );
      expect(failureNext(run)).toBe(
        "Start a new task, or ask for a short summary first.",
      );
      expect(failureCode(run)).toBe("provider_error 400");
      expect(
        failureNext({ ...run, providerMessage: "Invalid model: gpt-x" }),
      ).toBe(
        "Try again in a moment, or ask an administrator to check the model route.",
      );
    },
  );

  // The repeat guard stops at three failures with no change between, or at six in a run: one sentence for both, which
  // keys on the code, never on the runtime's reason text.
  it.each([
    "the same tool call failed three times with no change between",
    "the same tool call failed six times in this run",
  ])(
    "says the runtime's repeated failure in plain words, never its sentence (%s)",
    (reason) => {
      const run = { code: "repeated_failure", reason };
      expect(failureReason(run)).toBe(
        "The agent tried the same step several times and it kept failing",
      );
      expect(failureNext(run)).toBe("Send a message to try another way.");
      expect(failureCode(run)).toBeNull();
    },
  );

  // A text-route run whose retry after a copied-back correction was unreadable too fails on its code, whatever the
  // reason's suffix ("fallback_parse: echo" included): the run's words are the code's, the reason stays out of them.
  it("says a text-route failure on its code, the echo reason included", () => {
    for (const reason of [
      "fallback_parse: echo",
      "fallback_parse: malformed",
    ]) {
      const run = { code: "fallback_parse", reason };
      expect(failureReason(run)).toBe("The model's answer couldn't be used");
      expect(failureNext(run)).toBe(
        "Send the message again, or choose another model.",
      );
    }
  });

  // The same code tells the same story as its "Not sent" notice (failure.agent), with what to do next.
  it.each([
    [
      "runtime_start_failed",
      "The agent stopped while starting",
      "Send a message to try again; if it happens again, ask an administrator to rebuild the environment.",
    ],
    [
      "sandbox_failed",
      "The task's environment couldn't be started",
      "Send the message again; if it fails again, choose another environment or ask an administrator.",
    ],
  ])("words %s as its start notice does", (code, reason, next) => {
    const run = { code, reason: "diagnostic detail" };
    expect(failureReason(run)).toBe(reason);
    expect(failureNext(run)).toBe(next);
  });

  // A model call whose route key the gateway couldn't record the use of (GW-I-147): the relay refuses it (503) and the
  // run fails as provider_error. The runtime marks the gateway's own refusal (X-TQA-Gateway-Error) in the reason
  // ("gateway.key_use_unrecorded 503"), so the page says the key's sentence, never "The provider said" (the provider
  // said nothing), the relay's English or a code.
  it("words a model call refused because its key's use couldn't be recorded", () => {
    const run = {
      code: "provider_error",
      reason: "gateway.key_use_unrecorded 503",
      providerMessage:
        "The key could not be used: its use could not be recorded. Try again.",
    };
    expect(failureReason(run)).toBe(
      "The key could not be used because its use could not be recorded",
    );
    expect(failureNext(run)).toBe(
      "Try again in a moment; if it keeps happening, tell an administrator.",
    );
    expect(failureCode(run)).toBeNull();
    expect(failureProvider(run)).toBeNull();
    const transcript = [
      frame(1, { type: "message.user", text: "Summarise it", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      frame(3, {
        type: "run.failed",
        run_id: runId,
        code: "provider_error",
        reason: run.reason,
        provider_message: run.providerMessage,
      }),
    ].reduce(apply, emptyTranscript());
    const { container } = render(<TranscriptView transcript={transcript} />);
    expect(container.querySelector(".turn-outcome")).toHaveTextContent(
      "The key could not be used because its use could not be recorded. Try again in a moment; if it keeps happening, tell an administrator.",
    );
    expect(container).not.toHaveTextContent(
      /provider said|503|key_use_unrecorded|Try again\.$/,
    );
  });

  it("quotes no provider words on a retry the gateway refused", () => {
    const retried = [
      frame(1, { type: "message.user", text: "Summarise it", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      frame(3, { type: "step.started", step: 1, attempt: 1 }),
      frame(4, {
        type: "provider.retry",
        attempt: 1,
        reason: "gateway.key_use_unrecorded 503",
        delay_ms: 1000,
        provider_message:
          "The key could not be used: its use could not be recorded. Try again.",
      }),
    ].reduce(apply, emptyTranscript());
    const step = retried.items.find((item) => item.kind === "step");
    expect(step?.kind === "step" && step.retries).toEqual([
      { reason: "gateway.key_use_unrecorded 503", delayMs: 1000 },
    ]);
  });

  // Every code the model relay answers by itself (the gateway's relay/endpoint, request, rate, throttle and forward):
  // our server's refusal in plain words with what to do, never "the provider" and never the code.
  it.each([
    ["unauthorized", 401],
    ["forbidden", 403],
    ["not_found", 404],
    ["invalid_request", 400],
    ["body_too_large", 413],
    ["rate_limited", 429],
    ["restarting", 503],
    ["store_unavailable", 503],
    ["secret_unavailable", 503],
    ["key_use_unrecorded", 503],
    ["upstream_unavailable", 502],
    ["timeout", 504],
    ["configuration", 500],
  ])("words the model relay's own %s", (code, status) => {
    const run = {
      code: "provider_error",
      reason: `gateway.${code} ${status}`,
      providerMessage: "The gateway's own sentence.",
    };
    const known = knownFailure(run.code, run.reason);
    expect(known, code).not.toBeNull();
    expect(words(known?.reason ?? "")).toBeTruthy();
    expect(words(known?.next ?? "")).toBeTruthy();
    expect(failureReason(run)).not.toMatch(/provider|gateway|[0-9]{3}/i);
    // "restarting" may be an English word; "secret_unavailable" isn't.
    if (code.includes("_")) expect(failureReason(run)).not.toContain(code);
    expect(failureCode(run)).toBeNull();
    expect(failureProvider(run)).toBeNull();
  });

  it("says our server refused a gateway code it has no words for, with the code after", () => {
    const run = {
      code: "provider_error",
      reason: "gateway.something_new 503",
      providerMessage: "x",
    };
    expect(failureReason(run)).toBe(
      "The agent's server refused the model call",
    );
    expect(failureCode(run)).toBe("something_new");
    expect(failureProvider(run)).toBeNull();
  });

  // Without the gateway's mark, the same word is the provider's own failure (the runtime's LlmError codes).
  it("keeps an unmarked rate limit as the provider's", () => {
    const run = {
      code: "provider_error",
      reason: "rate_limited 429",
      providerMessage: "Slow down",
    };
    expect(failureReason(run)).toBe("The model provider refused the request");
    expect(failureProvider(run)).toBe("Slow down");
    expect(failureCode(run)).toBe("rate_limited 429");
  });

  // Any other provider failure keeps the provider's own words first (WEB-U-055).
  it("keeps a real provider's own words", () => {
    const run = {
      code: "provider_error",
      reason: "http 400",
      providerMessage: "Invalid model: gpt-x",
    };
    expect(failureProvider(run)).toBe("Invalid model: gpt-x");
    expect(failureCode(run)).toBe("http 400");
  });

  it("shows an unknown code after a plain sentence, never its raw reason", () => {
    const run = { code: "mystery_code", reason: "internal.mystery" };
    expect(failureReason(run)).toBe("The run stopped");
    expect(failureCode(run)).toBe("mystery_code");
    const transcript = [
      frame(1, { type: "message.user", text: "Do it", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      frame(3, {
        type: "run.failed",
        run_id: runId,
        code: "internal",
        reason: "ignored",
      }),
    ].reduce(apply, emptyTranscript());
    render(<TranscriptView transcript={transcript} />);
    expect(document.body).not.toHaveTextContent("ignored");
    expect(
      screen.getByText("Something went wrong inside the agent."),
    ).toBeVisible();
    expect(english("summary.code", { code: "mystery_code" })).toBe(
      "Code: mystery_code",
    );
  });
});
