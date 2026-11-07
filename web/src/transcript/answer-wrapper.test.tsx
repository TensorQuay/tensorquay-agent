import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame, runId } from "../test/frames";
import { settleLazy } from "../test/lazy";

import { unwrapAnswer } from "./answer-wrapper";
import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

// C2: some models (qwen) wrap their final answer, or its closing paragraph, in <result>…</result> or <note>…</note>.
// The page shows the inner text; a tag anywhere else is the model's own text and stays as it is.
describe("WEB-U-135 a final answer's wrapper tag", () => {
  it.each([
    [
      "the whole answer in result",
      "<result>The totals match.</result>",
      "The totals match.",
    ],
    [
      "the whole answer in note",
      "<note>Nothing changed.</note>",
      "Nothing changed.",
    ],
    [
      "a whole answer over several paragraphs, with space around the tags",
      "\n<result>\nFirst.\n\nSecond.\n</result>\n",
      "First.\n\nSecond.",
    ],
    [
      "a trailing paragraph",
      "I compared the sheets.\n\n<note>Region C has no rows yet.</note>",
      "I compared the sheets.\n\nRegion C has no rows yet.",
    ],
    [
      "a trailing paragraph after a closed code block",
      "Run it:\n\n```sh\npython3 week.py\n```\n\n<result>It prints 30 lines.</result>",
      "Run it:\n\n```sh\npython3 week.py\n```\n\nIt prints 30 lines.",
    ],
  ])("unwraps %s", (_, text, shown) => {
    expect(unwrapAnswer(text)).toBe(shown);
  });

  it.each([
    ["a tag mid-text", "Use <result> to mark it.", "Use <result> to mark it."],
    [
      "a wrapped paragraph that isn't the last",
      "<note>First.</note>\n\nThen more.",
      "<note>First.</note>\n\nThen more.",
    ],
    [
      "a tag inside a paragraph's text",
      "The answer is <result>42</result>.",
      "The answer is <result>42</result>.",
    ],
    [
      "a whole code block holding the tags",
      "```xml\n<result>42</result>\n```",
      "```xml\n<result>42</result>\n```",
    ],
    [
      "tags at the end of an open code block",
      "Example:\n\n```xml\n<a/>\n\n<result>42</result>",
      "Example:\n\n```xml\n<a/>\n\n<result>42</result>",
    ],
    [
      "a code block's last paragraph",
      "```\none\n\n<note>two</note>\n```",
      "```\none\n\n<note>two</note>\n```",
    ],
    [
      "two wrapped paragraphs, the tags not one pair",
      "<result>A</result>\n\n<result>B</result>",
      "<result>A</result>\n\nB",
    ],
    ["another tag", "<answer>42</answer>", "<answer>42</answer>"],
    ["an unclosed tag", "<result>42", "<result>42"],
    ["mismatched tags", "<result>42</note>", "<result>42</note>"],
    ["an empty wrapper", "<result></result>", "<result></result>"],
  ])("leaves %s as it is", (_, text, shown) => {
    expect(unwrapAnswer(text)).toBe(shown);
  });
});

describe("WEB-U-135 on the page", () => {
  const answer = (text: string) =>
    [
      frame(1, { type: "message.user", text: "Check it", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      frame(3, { type: "step.started", step: 1 }),
      frame(4, { type: "text.delta", text }),
      frame(5, { type: "step.completed", step: 1, duration_ms: 300 }),
      frame(6, { type: "run.completed", run_id: runId }),
    ].reduce(apply, emptyTranscript());

  it("shows a wrapped final answer without its tag", async () => {
    const { container } = render(
      <TranscriptView
        transcript={answer("Checked.\n\n<result>The totals match.</result>")}
      />,
    );
    await settleLazy();
    expect(await screen.findByText("The totals match.")).toBeVisible();
    expect(container).not.toHaveTextContent(/<\/?result>/);
  });

  it("leaves a code block's tags in it", async () => {
    render(
      <TranscriptView
        transcript={answer("```xml\n<result>42</result>\n```")}
      />,
    );
    await settleLazy();
    const code = await screen.findByText(
      (_, element) =>
        element?.tagName === "CODE" &&
        element.textContent === "<result>42</result>\n",
    );
    expect(code).toBeVisible();
  });
});
