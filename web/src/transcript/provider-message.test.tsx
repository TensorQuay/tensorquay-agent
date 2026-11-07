import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame } from "../test/frames";

import { failureReason } from "./failure-copy";
import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

const RTL = "המפתח אינו תקף";
const MARKUP =
  '<img src=x onerror="alert(1)"> **bold** [link](https://example.invalid)';

function run(retry?: string, failure?: string) {
  return [
    frame(1, { type: "message.user", text: "Summarize", attachments: [] }),
    frame(2, { type: "run.started", run_id: "r-1" }),
    frame(3, {
      type: "provider.retry",
      attempt: 1,
      reason: "http 503",
      delay_ms: 2000,
      ...(retry === undefined ? {} : { provider_message: retry }),
    }),
    frame(4, {
      type: "run.failed",
      run_id: "r-1",
      code: "provider_error",
      reason: "provider_error 400",
      ...(failure === undefined ? {} : { provider_message: failure }),
    }),
  ].reduce(apply, emptyTranscript());
}

// LLM-U-036, CORE-U-052, PROTO-U-019: a failed run and a retry say what the provider said, as
// its own words next to the classification, escaped and isolated, never as Markdown or HTML (WEB-U-055).
describe("WEB-U-055 the provider's own words on a failure and a retry", () => {
  it("leads a failed run with them, keeps the code secondary, and shows them with the retry in its step", () => {
    const { container } = render(
      <TranscriptView
        transcript={run("Overloaded, try later", "Invalid model: gpt-x")}
      />,
    );
    // UX review 0927b, item 4: the provider's words first, in plain English; the code after, small and muted.
    const outcome = container.querySelector(".turn-outcome");
    expect(outcome).toHaveTextContent(
      /^The provider said: Invalid model: gpt-x Try again in a moment, or ask an administrator to check the model route\./,
    );
    const code = within(outcome as HTMLElement).getByText(
      "Code: provider_error 400",
    );
    expect(code).toHaveClass("muted");
    // The header's "Failed: …" reads the same plain words.
    expect(
      failureReason({ code: "provider_error", reason: "provider_error 400" }),
    ).toBe("The model provider refused the request");
    // The retry sits in its step, in plain words with the provider's own (the failed turn's group stays open).
    expect(
      screen.getByText("The model server answered with an error; trying again.")
        .parentElement,
    ).toHaveTextContent("The provider said: Overloaded, try later");
  });

  it("keeps markup-like text literal and isolates right-to-left text", () => {
    const { container } = render(
      <TranscriptView transcript={run(RTL, MARKUP)} />,
    );
    const said = [...container.querySelectorAll("bdi")];
    // Both the retry's words (in its step) and the failure's, whichever comes first on the page.
    expect(said.map((element) => element.textContent).sort()).toEqual(
      [RTL, MARKUP].sort(),
    );
    for (const element of said) {
      expect(element).toHaveAttribute("dir", "auto");
      expect(element.children).toHaveLength(0);
    }
    expect(container.querySelector("img, strong, a[href]")).toBeNull();
  });

  it("says a provider failure in plain words when the provider said nothing", () => {
    const { container } = render(<TranscriptView transcript={run()} />);
    expect(container.querySelector("bdi")).toBeNull();
    expect(screen.queryByText(/The provider said/)).toBeNull();
    expect(container.querySelector(".turn-outcome")).toHaveTextContent(
      "The model provider refused the request. Try again in a moment, or ask an administrator to check the model route. Code: provider_error 400",
    );
  });
});
