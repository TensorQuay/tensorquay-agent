import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame, transcriptFrames } from "../test/frames";

import { present } from "./present";
import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

describe("WEB-U-018 recorded environment changes", () => {
  const initial = frame(1, {
    type: "environment.changed",
    template: "minimal",
    capabilities: ["shell"],
  });

  it("hides the initial environment live and on replay while retaining its recorded identity", () => {
    const transcript = apply(emptyTranscript(), initial);
    const { rerender } = render(<TranscriptView transcript={transcript} />);
    expect(screen.queryByText(/^Switched to /)).not.toBeInTheDocument();
    expect(transcript.items[0]).toMatchObject({ template: "minimal" });
    const replay = [
      initial,
      frame(2, {
        type: "message.user",
        text: "Start a project",
        attachments: [],
      }),
    ].reduce(apply, emptyTranscript());
    rerender(<TranscriptView transcript={replay} />);
    expect(screen.getByText("Start a project")).toBeVisible();
    expect(screen.queryByText(/^Switched to /)).not.toBeInTheDocument();
  });

  it("places a later environment change after the preceding answer and summary", () => {
    const transcript = apply(
      transcriptFrames
        .map((value) =>
          "seq" in value && value.seq != null
            ? { ...value, seq: value.seq + 1 }
            : value,
        )
        .reduce(apply, apply(emptyTranscript(), initial)),
      frame(21, {
        type: "environment.changed",
        template: "workstation",
        capabilities: ["shell"],
      }),
    );
    const turns = present(transcript);
    expect(turns).toHaveLength(2);
    expect(turns[0]?.notices).toHaveLength(0);
    expect(turns[1]).toMatchObject({
      prompt: null,
      answer: [],
      summary: null,
      notices: [{ template: "workstation" }],
    });
  });

  it("shows the first recorded change when an earlier run used the implicit environment", () => {
    const transcript = apply(
      transcriptFrames.reduce(apply, emptyTranscript()),
      frame(20, {
        type: "environment.changed",
        template: "workstation",
        capabilities: ["shell"],
      }),
    );
    const turns = present(transcript);
    expect(turns).toHaveLength(2);
    expect(turns[1]?.notices).toEqual([
      expect.objectContaining({ template: "workstation" }),
    ]);
    render(
      <TranscriptView
        transcript={transcript}
        templateNames={{ workstation: "Workstation" }}
      />,
    );
    expect(screen.getByText("Switched to Workstation")).toBeVisible();
  });

  it("uses the catalogue display name while preserving the recorded template identity", () => {
    const transcript = apply(
      apply(emptyTranscript(), initial),
      frame(2, {
        type: "environment.changed",
        template: "workstation",
        capabilities: ["shell", "python", "browser"],
      }),
    );
    render(
      <TranscriptView
        transcript={transcript}
        templateNames={{ workstation: "Workstation" }}
      />,
    );
    expect(screen.getByText("Switched to Workstation")).toBeVisible();
    expect(transcript.items[1]).toMatchObject({ template: "workstation" });
  });

  it("keeps an old recorded change readable when its template is no longer in the catalogue", () => {
    const transcript = apply(
      apply(emptyTranscript(), initial),
      frame(2, {
        type: "environment.changed",
        template: "retired-template",
        capabilities: ["shell"],
      }),
    );
    render(<TranscriptView transcript={transcript} templateNames={{}} />);
    expect(screen.getByText("Switched to retired-template")).toBeVisible();
  });
});
