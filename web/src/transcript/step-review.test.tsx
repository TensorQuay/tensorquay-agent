import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame, runId } from "../test/frames";
import { settleLazy } from "../test/lazy";

import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

const ls = [
  frame(1, { type: "message.user", text: "List", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, {
    type: "provider.retry",
    attempt: 1,
    reason: "timeout",
    delay_ms: 1000,
  }),
  frame(4, {
    type: "provider.retry",
    attempt: 2,
    reason: "timeout",
    delay_ms: 1000,
  }),
  frame(5, {
    type: "tool.call",
    call_id: "c-1",
    name: "shell",
    arguments: { command: "ls" },
  }),
  frame(6, { type: "tool.start", call_id: "c-1" }),
  frame(7, {
    type: "tool.output",
    call_id: "c-1",
    chunk: 0,
    data: "README.md\nsrc/",
    last: true,
  }),
  frame(8, { type: "tool.end", call_id: "c-1", status: "ok", exit_code: 0 }),
  frame(9, {
    type: "tool.result",
    call_id: "c-1",
    status: "ok",
    content: "README.md\nsrc/",
  }),
  frame(10, { type: "text.delta", text: "Two entries." }),
  frame(11, { type: "run.completed", run_id: runId }),
];
const show = (frames: ReturnType<typeof frame>[]) =>
  render(
    <TranscriptView transcript={frames.reduce(apply, emptyTranscript())} />,
  );

// WEB-U-016: one item per call, in words.
describe("WEB-U-016 one item per call, in words", () => {
  it("T-a/T-b: the row is the card's heading; output has its own label; arguments stay behind Details; no internal name or Completed badge", async () => {
    show(ls);
    fireEvent.click(screen.getByRole("button", { name: /Worked for/ }));
    await settleLazy();
    const card = screen.getByRole("article", { name: "shell tool call" });
    expect(within(card).queryByRole("button", { name: /shell/ })).toBeNull();
    expect(card.querySelector(".tool-name")).toBeNull();
    expect(within(card).queryByText("Completed")).toBeNull();
    const output = within(card).getByRole("group", { name: "Result" });
    expect(output).toHaveTextContent("README.md");
    expect(
      within(card).getByText("Output", { selector: ".type-label" }),
    ).toBeVisible();
    // The arguments are a closed Details toggle after the output, never a bare label.
    const details = within(card).getByRole("button", { name: "Details" });
    expect(details).toHaveAttribute("aria-expanded", "false");
    expect(
      within(card).queryByRole("group", { name: "All arguments" }),
    ).toBeNull();
    expect(
      output.compareDocumentPosition(details) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    fireEvent.click(details);
    expect(details).toHaveAttribute("aria-expanded", "true");
    expect(
      within(card).getByRole("group", { name: "All arguments" }),
    ).toHaveTextContent('"command": "ls"');
  });

  it("offers no Details for a call without arguments", async () => {
    show(
      ls.map((value) =>
        value.event.type === "tool.call"
          ? { ...value, event: { ...value.event, arguments: {} } }
          : value,
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: /Worked for/ }));
    await settleLazy();
    const card = screen.getByRole("article", { name: "shell tool call" });
    expect(within(card).queryByRole("button", { name: "Details" })).toBeNull();
    expect(within(card).queryByText("All arguments")).toBeNull();
  });

  it("turns the row's chevron with its card, one item open or closed", async () => {
    show(ls);
    fireEvent.click(screen.getByRole("button", { name: /Worked for/ }));
    await settleLazy();
    const row = screen.getByRole("button", { name: /^Ran a command: ls/ });
    const line = row.closest(".activity-line") as HTMLElement;
    expect(row).toHaveAttribute("aria-expanded", "true");
    expect(line).toHaveAttribute("data-open");
    fireEvent.click(row);
    expect(row).toHaveAttribute("aria-expanded", "false");
    expect(line).not.toHaveAttribute("data-open");
  });

  it("T-c: recovered retries are said on the step's summary line, not under the command", () => {
    show(ls);
    const heading = screen.getByRole("button", { name: /Worked for/ });
    expect(heading).toHaveTextContent(
      /ran 1 command · the model was slow, retried twice$/,
    );
    fireEvent.click(heading);
    expect(screen.queryByText("Retried twice")).toBeNull();
  });

  it("T-e: the footer says only what changed; the rest waits in its tooltip", () => {
    const { container } = show(ls);
    const footer = container.querySelector(".turn-summary");
    expect(footer).toHaveTextContent(/^No files changed$/);
    expect(footer?.getAttribute("title")).toMatch(/steps?.*command.*tokens/);
  });

  // WEB-U-461: the tooltip gives the token counts exactly, as a short count's does, and screen readers get the same
  // details, which a title alone reaches few of; the footer still shows only what changed.
  it("gives the footer's details exactly, on hover and to screen readers", () => {
    const { container } = show([
      ...ls.slice(0, 10),
      frame(11, {
        type: "usage",
        prompt_tokens: 218469,
        completion_tokens: 6269,
      }),
      frame(12, { type: "run.completed", run_id: runId }),
    ]);
    const footer = container.querySelector(".turn-summary");
    expect(footer).toHaveTextContent(/^No files changed$/);
    const details = footer?.getAttribute("title") ?? "";
    expect(details).toMatch(
      / · 218\u202f469 input · 6\u202f269 output tokens$/,
    );
    // Matched as is: the default matcher would turn the narrow spaces that group the figures into plain ones.
    const spoken = screen.getByText(details, { normalizer: (text) => text });
    expect(spoken).toHaveClass("sr-only");
    expect(footer).not.toContainElement(spoken);
  });
});
