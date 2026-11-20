import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { RunStatus } from "./RunStatus";

// A stand-in presentation for the first case only: a status with Done's "ok" tone that isn't Done.
const stand = vi.hoisted(() => ({ on: false }));
vi.mock("./run-status", async (original) => {
  const real = await original<typeof import("./run-status")>();
  return {
    ...real,
    runPresentation: (...args: Parameters<typeof real.runPresentation>) =>
      stand.on
        ? {
            label: "Checked",
            word: "Checked",
            elapsed: null,
            environment: null,
            tone: "ok",
            took: null,
          }
        : real.runPresentation(...args),
  };
});

const finished = [
  frame(1, { type: "message.user", text: "List the files", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, { type: "run.completed", run_id: runId }),
].reduce(apply, emptyTranscript());

// WEB-U-495 (139b): the header hides Done because runPresentation says the run finished, never because of a tone a
// future status might share; and a team's words laid over a finished run are that status, shown.
describe("WEB-U-495 Done by its flag", () => {
  it("keeps a status with Done's tone that isn't Done on the screen", () => {
    stand.on = true;
    try {
      render(<RunStatus transcript={finished} />);
      expect(screen.getByRole("status")).toHaveTextContent("Checked");
      expect(screen.getByRole("status")).not.toHaveClass("sr-only");
    } finally {
      stand.on = false;
    }
  });

  it("shows a lead's waiting words over its finished run", () => {
    render(
      <RunStatus transcript={finished} leadWaiting="Waiting for the team" />,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Waiting for the team",
    );
    expect(screen.getByRole("status")).not.toHaveClass("sr-only");
  });
});
