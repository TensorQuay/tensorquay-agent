import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame } from "../test/frames";

import { PUSH_FAILURES } from "./push-failure";
import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

const push = (content: string) =>
  [
    frame(1, { type: "step.started", step: 1, attempt: 1 }),
    frame(2, {
      type: "tool.call",
      call_id: "call-push",
      name: "git_push",
      arguments: {
        branch: "feature/readme",
        remote: "https://git.example.invalid/team/demo.git",
      },
    }),
    frame(3, { type: "tool.start", call_id: "call-push" }),
    frame(4, { type: "tool.end", call_id: "call-push", status: "error" }),
    frame(5, {
      type: "tool.result",
      call_id: "call-push",
      status: "error",
      content,
    }),
  ].reduce(apply, emptyTranscript());

// Every way a push can fail (GitPushFailure, typed so a new one without words fails the typecheck) says what happened
// and what to do under its step, in plain words with no git words and no code; the two credential failures keep their
// link to Keys (WEB-U-038).
describe("WEB-U-094 every failed push in plain words", () => {
  it.each(Object.entries(PUSH_FAILURES).filter(([, words]) => words !== "key"))(
    "words %s",
    (code, words) => {
      const { container } = render(
        <TranscriptView transcript={push(`${code}: the tool's own detail`)} />,
      );
      const hint = container.querySelector(".activity-next");
      expect(hint).not.toBeNull();
      expect(hint?.textContent).toBeTruthy();
      if (code.includes("_")) expect(hint?.textContent).not.toContain(code);
      expect(hint?.textContent).not.toMatch(/\bgit\b|\bpush\b|branch/i);
      expect(words).toMatch(/^activity\.push/);
    },
  );

  // a0718b8c (GW-I-149): the key's use couldn't be recorded, so nothing was saved.
  it("says a push whose key's use couldn't be recorded didn't run", () => {
    render(
      <TranscriptView
        transcript={push(
          "key_use_unrecorded: The push didn't run: the key's use couldn't be recorded. Try again in a moment.",
        )}
      />,
    );
    expect(
      screen.getByText(
        "The changes weren't saved: the key's use couldn't be recorded. Try again in a moment.",
      ),
    ).toHaveClass("activity-next");
  });

  it("adds nothing for a code this page doesn't know, beyond the step's own line", () => {
    const { container } = render(
      <TranscriptView transcript={push("brand_new_code: something")} />,
    );
    expect(container.querySelector(".activity-next")).toBeNull();
    expect(
      screen.getByRole("button", { name: /^Couldn't save the changes/ }),
    ).toBeVisible();
  });
});
