import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { ErrorCode } from "../protocol/generated/protocol";
import { frame } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

const RTL = "המנוע עצר כאן";
const MARKUP =
  '<img src=x onerror="alert(1)"> **bold** [link](https://example.invalid)';

function stopped(code: ErrorCode, reason: string) {
  return [
    frame(1, { type: "message.user", text: "Do it", attachments: [] }),
    frame(2, { type: "run.started", run_id: "r-1" }),
    frame(3, { type: "run.failed", run_id: "r-1", code, reason }),
  ].reduce(apply, emptyTranscript());
}

function outcome(code: ErrorCode, reason: string) {
  const { container } = render(
    <TranscriptView transcript={stopped(code, reason)} />,
  );
  return container.querySelector(".turn-outcome") as HTMLElement;
}

// WEB-U-328 (WEB-E2E-070, ADR-0013's `Stop { reason }`): a run a decision engine stopped keeps its plain sentence and
// adds the engine's own reason after it, quoted, as text only, isolated and bounded, the way a provider's words are
// shown (WEB-U-055). Every other failure's recorded reason stays a diagnostic, never shown.
describe("WEB-U-328 a decision engine's stop shows its reason", () => {
  it("keeps the plain sentence and adds the engine's words, quoted and isolated", () => {
    const line = outcome("decision_stop", "The test engine stops here.");
    expect(line).toHaveTextContent(
      /^The decision engine stopped the task\. The engine said: The test engine stops here\./,
    );
    const words = line.querySelector(".engine-words bdi[dir='auto'] q");
    expect(words).toHaveTextContent(/^The test engine stops here\.$/);
  });

  it("shows the words as text, never Markdown or HTML, and isolates right-to-left text", () => {
    const line = outcome("decision_stop", `${RTL} ${MARKUP}`);
    expect(line.querySelector("img")).toBeNull();
    expect(line.querySelector("a")).toBeNull();
    expect(line.querySelector("strong")).toBeNull();
    expect(line.querySelector(".engine-words bdi q")).toHaveTextContent(
      `${RTL} ${MARKUP}`,
    );
  });

  it("bounds a long reason", () => {
    const line = outcome("decision_stop", "x".repeat(500));
    const words = line.querySelector(".engine-words q")?.textContent ?? "";
    expect(words).toBe(`${"x".repeat(200)}…`);
  });

  it("shows nothing more when the engine gave no reason", () => {
    const line = outcome("decision_stop", "  ");
    expect(line.querySelector(".engine-words")).toBeNull();
    expect(line).toHaveTextContent(/^The decision engine stopped the task\./);
  });

  it("leaves every other failure's recorded reason unshown", () => {
    const line = outcome("internal", "a diagnostic for the logs");
    expect(line.querySelector(".engine-words")).toBeNull();
    expect(document.body).not.toHaveTextContent("a diagnostic for the logs");
  });
});
