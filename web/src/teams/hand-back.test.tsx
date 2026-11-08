import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { TeamMessageLine } from "./TeamMessageLine";

const words =
  "The gateway hands back researcher's work: researcher stopped before it reported (the team was stopped). Attached: 2 files it changed in its own copy; take them with take_files.";

// WEB-U-403 (a live check before 17.0, GW-I-311): a helper stopped before it reported is handed back by the gateway, in
// the gateway's words. The log line reads as the gateway's own, with its files, never as "<helper> reported".
describe("WEB-U-403 the gateway's hand-back in the team's log", () => {
  it("reads as the gateway's words with its files", () => {
    render(
      <TeamMessageLine
        message={{
          seq: 9,
          type: "report",
          from: { kind: "gateway" },
          to: { kind: "lead" },
          text: words,
          files: 2,
        }}
      />,
    );
    const line = screen.getByRole("button");
    expect(line).toHaveTextContent(`${words} · 2 files`);
    expect(line.textContent?.startsWith("The gateway hands back")).toBe(true);
    expect(line).not.toHaveTextContent(" reported:");
  });

  it("reads alone when nothing is attached", () => {
    render(
      <TeamMessageLine
        message={{
          seq: 10,
          type: "report",
          from: { kind: "gateway" },
          to: { kind: "lead" },
          text: "The gateway hands back writer's work: writer stopped before it reported (the team was stopped). It worked in your own files, so its changes are already there: nothing to attach.",
        }}
      />,
    );
    expect(
      screen
        .getByRole("button")
        .textContent?.startsWith("The gateway hands back"),
    ).toBe(true);
  });
});
