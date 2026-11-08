import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

const retry = (seq: number, reason: string, delay = 1203) =>
  frame(seq, {
    type: "provider.retry",
    attempt: seq,
    reason,
    delay_ms: delay,
  });
const start = [
  frame(1, { type: "message.user", text: "Summarize", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, { type: "step.started", step: 1 }),
];
const show = (frames: ReturnType<typeof frame>[]) =>
  render(
    <TranscriptView transcript={frames.reduce(apply, emptyTranscript())} />,
  );

// The review's finding 15 and plan §3: a retry that worked is not news. It belongs to its step, in plain words while
// the agent works, then one quiet "Retried twice" inside the step's details; the code stays in a tooltip.
describe("WEB-U-016 a model retry is part of its step, not a transcript notice", () => {
  it("says what happened in plain words while the agent works, without codes or false precision", () => {
    const { container } = show([...start, retry(4, "fallback_parse: schema")]);
    const group = screen.getByRole("group", { name: "Activity" });
    const line = within(group).getByText(
      "The model's reply couldn't be read; trying again.",
    );
    expect(line).toHaveAttribute(
      "title",
      "The model's reply couldn't be read. Details: fallback_parse: schema",
    );
    expect(container).not.toHaveTextContent(
      /fallback_parse|1\.203|retrying in/,
    );
    expect(container.querySelector(".transcript-notice")).toBeNull();
  });

  it("folds recovered retries into one line in the step's details once the run is done", () => {
    const { container } = show([
      ...start,
      retry(4, "http 503"),
      retry(5, "timeout"),
      frame(6, { type: "text.delta", text: "Done." }),
      frame(7, { type: "run.completed", run_id: runId }),
    ]);
    expect(container).not.toHaveTextContent(
      /Model request failed|trying again/,
    );
    // Counted on the step's summary line (finding T-c), by the last reason; the codes in its tooltip.
    const heading = screen.getByRole("button", { name: /Worked for/ });
    expect(heading).toHaveTextContent(/the model was slow, retried twice$/);
    expect(heading.querySelector("[title]")).toHaveAttribute(
      "title",
      "The model server answered with an error. The model took too long to answer. Details: http 503, timeout",
    );
  });

  // CORE-U-062, the link-aware retry: the runtime waited for its own link to the gateway, spending no attempt, and reports it
  // once the link is back. The model call was lost with the link, so the line says the step starts over (a first-hour
  // check): whole seconds, at least 1; never "Model request failed", and never a bare "Reconnected".
  it("says a retry after the gateway link came back asks the model again from the start, in whole seconds", () => {
    const words =
      "The connection to the server dropped for 1 s, so the model was asked again and this step started over.";
    const { container } = show([...start, retry(4, "gateway_link", 1400)]);
    expect(screen.getByText(words)).toHaveAttribute(
      "title",
      "The connection to the server dropped. Details: gateway_link",
    );
    expect(container).not.toHaveTextContent(
      /Model request failed|trying again|Reconnected/,
    );
    cleanup();
    show([...start, retry(4, "gateway_link", 200)]);
    expect(screen.getByText(words)).toBeInTheDocument();
  });

  it.each([
    ["http 503", "The model server answered with an error"],
    ["rate_limited", "The model server asked to slow down"],
    ["unavailable", "The model server was unavailable"],
    ["timeout", "The model took too long to answer"],
    ["stream_cut", "The model's answer was cut off"],
    ["protocol", "The model's reply couldn't be read"],
    ["tool_args_invalid", "The model's reply couldn't be read"],
    ["io", "The model server couldn't be reached"],
    // CORE-U-065: the server hid a tool call; the step asks again with the tools declared.
    ["native_tools withheld=26", "The model server hid a tool call"],
    ["native_tools refused", "The model server hid a tool call"],
    ["something_new", "The model request failed"],
    // The text route refused a reply that only copied its correction back (the runtime's exact reason); any other
    // fallback_parse, an echo-like one included, reads as unreadable.
    [
      "fallback_parse: echo",
      "The model repeated an instruction instead of answering",
    ],
    ["fallback_parse: echoed", "The model's reply couldn't be read"],
  ])("reads %s as “%s”", (reason, words) => {
    show([...start, retry(4, reason)]);
    expect(screen.getByText(`${words}; trying again.`)).toBeInTheDocument();
  });
});

// WEB-U-304, from the c14b-rc wave evidence: a step retried for an unreadable reply had "fallback_parse: malformed" alone in
// its tooltip, which read as the explanation. The tooltip leads with the words; the codes follow a "Details:" label.
describe("WEB-U-304 a retry's tooltip says it in words before the runtime's codes", () => {
  it("names each reason once and each code once, in order, after the done step's count", () => {
    show([
      ...start,
      retry(4, "fallback_parse: malformed"),
      retry(5, "fallback_parse: malformed"),
      retry(6, "tool_args_invalid"),
      retry(7, "something_new"),
      frame(8, { type: "text.delta", text: "Done." }),
      frame(9, { type: "run.completed", run_id: runId }),
    ]);
    const heading = screen.getByRole("button", { name: /Worked for/ });
    expect(heading.querySelector("[title]")).toHaveAttribute(
      "title",
      "The model's reply couldn't be read. The model request failed. Details: fallback_parse: malformed, tool_args_invalid, something_new",
    );
  });

  // The 7 Oct review's P3: a title is for a pointer only, so keyboard and touch never read the reasons. As the team
  // sheet carries the strip's exact figures, the opened step carries them in one quiet line, describing its toggle.
  it("puts the done step's reasons and codes in the step too, read with its toggle by any means", () => {
    show([
      ...start,
      retry(4, "http 503"),
      retry(5, "fallback_parse: echo"),
      frame(6, { type: "text.delta", text: "Done." }),
      frame(7, { type: "run.completed", run_id: runId }),
    ]);
    // The summary says the last reason ("the model repeated an instruction, retried twice"); the line, the rest.
    const words =
      "The model server answered with an error. Details: http 503, fallback_parse: echo";
    const heading = screen.getByRole("button", { name: /Worked for/ });
    expect(heading).toHaveAccessibleDescription(words);
    expect(heading).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(heading);
    expect(screen.getByText(words)).toBeVisible();
    expect(screen.getAllByText(words)).toHaveLength(1);
  });

  // A pass over the pages before 17.0: the summary already says the reason
  // ("… · the model's reply couldn't be read, retried once"), so the line under it, and the toggle's description, never
  // say it again: only the codes after "Details:".
  it("says the summary's reason once: the step's line keeps only what the summary doesn't say", () => {
    show([
      ...start,
      retry(4, "fallback_parse: malformed"),
      frame(5, { type: "text.delta", text: "Done." }),
      frame(6, { type: "run.completed", run_id: runId }),
    ]);
    const heading = screen.getByRole("button", { name: /Worked for/ });
    expect(heading).toHaveTextContent(
      /the model's reply couldn't be read, retried once$/,
    );
    expect(heading).toHaveAccessibleDescription(
      "Details: fallback_parse: malformed",
    );
    fireEvent.click(heading);
    expect(screen.getAllByText(/couldn't be read/)).toHaveLength(1);
    // The pointer's tooltip on the summary keeps the whole: words, then codes.
    expect(heading.querySelector("[title]")).toHaveAttribute(
      "title",
      "The model's reply couldn't be read. Details: fallback_parse: malformed",
    );
  });

  it("describes no toggle and adds no line for a step done without retries, or while the agent works", () => {
    const done = show([
      ...start,
      frame(4, { type: "text.delta", text: "Done." }),
      frame(5, { type: "run.completed", run_id: runId }),
    ]);
    expect(done.container.querySelector("[aria-describedby]")).toBeNull();
    expect(done.container).not.toHaveTextContent("Details:");
    cleanup();
    const { container } = show([
      ...start,
      retry(4, "fallback_parse: malformed"),
    ]);
    expect(container).not.toHaveTextContent(/Details:|fallback_parse/);
  });

  it("never shows a code on its own while the agent works", () => {
    show([...start, retry(4, "fallback_parse: malformed")]);
    const title =
      screen
        .getByText("The model's reply couldn't be read; trying again.")
        .getAttribute("title") ?? "";
    expect(title).toMatch(/^The model's reply couldn't be read\. Details: /);
    expect(title.endsWith("fallback_parse: malformed")).toBe(true);
  });

  it("says a copied-back correction in its own words, on the line, the summary and the tooltip", () => {
    show([
      ...start,
      retry(4, "fallback_parse: echo"),
      frame(5, { type: "text.delta", text: "Done." }),
      frame(6, { type: "run.completed", run_id: runId }),
    ]);
    const heading = screen.getByRole("button", { name: /Worked for/ });
    expect(heading).toHaveTextContent(
      /the model repeated an instruction, retried once$/,
    );
    expect(heading.querySelector("[title]")).toHaveAttribute(
      "title",
      "The model repeated an instruction instead of answering. Details: fallback_parse: echo",
    );
  });
});
