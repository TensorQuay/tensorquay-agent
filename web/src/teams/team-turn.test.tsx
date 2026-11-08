import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { english } from "../i18n/translate";
import type { Team } from "../protocol/generated/api";
import { ready } from "../test/frames";
import { apply, emptyTranscript } from "../transcript/reducer";
import { RunStatus } from "../transcript/RunStatus";

import { teamView } from "./binding";
import { helperStateWords } from "./team-words";

// WEB-U-393 (a review and real-world use before 17.0, 17 Oct 2026): a lead waiting for its own helpers to finish on its
// files read "Waiting for a free slot · next", then "Waiting for the workspace", with 4 of 8 slots free; the strip said
// a my_files helper waited "for a slot"; and at 1280 with the files panel open the header was cut to "Waiting for th…".
// A team's turn on the lead's files says so, and the header shows "Waiting…", the whole in its tooltip and read.
const waiting = apply(emptyTranscript(), {
  ...ready,
  sandbox: { backend: "docker", state: "queued" },
  queue: { ahead: 0, reason: "workspace" },
});

describe("WEB-U-393 a team's turn on the lead's files, in words", () => {
  it("says a lead waits for a helper to finish with the files, short in the header", () => {
    const whole = "Waiting for a helper to finish with the files";
    render(<RunStatus transcript={waiting} team="lead" />);
    const group = screen.getByRole("group", { name: "Run status" });
    expect(screen.getByRole("status")).toHaveTextContent(whole);
    expect(group).toHaveAttribute("data-short", "Waiting…");
    expect(group).toHaveAttribute("title", whole);
  });

  it("says a helper waits for its turn on the lead's files", () => {
    render(<RunStatus transcript={waiting} team="member" />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Waiting for its turn on the lead's files",
    );
  });

  it("keeps the workspace's words outside a team, short in the header too", () => {
    render(<RunStatus transcript={waiting} />);
    const group = screen.getByRole("group", { name: "Run status" });
    expect(screen.getByRole("status")).toHaveTextContent(
      "Waiting for the workspace",
    );
    expect(group).toHaveAttribute("data-short", "Waiting…");
  });

  it("never calls a slot wait a turn on the files", () => {
    const slot = apply(emptyTranscript(), {
      ...ready,
      sandbox: { backend: "docker", state: "queued" },
      queue: { ahead: 1, reason: "slot" },
    });
    render(<RunStatus transcript={slot} team="lead" />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Waiting for a free slot · 1 ahead",
    );
  });

  it("words a my_files helper with no place in the slot queue as waiting its turn", () => {
    expect(
      helperStateWords({ state: "queued", workspace: "my_files" }, english),
    ).toEqual({
      words: "Waiting for its turn on the lead's files",
      tone: "muted",
    });
    // A place in the slot queue is a slot wait, whatever its files.
    expect(
      helperStateWords(
        { state: "queued", workspace: "my_files", position: 1 },
        english,
      ).words,
    ).toBe("Waiting for a slot (next)");
    expect(
      helperStateWords({ state: "queued", workspace: "own_copy" }, english)
        .words,
    ).toBe("Waiting for a slot");
  });

  it("keeps each helper's files from the gateway's answer", () => {
    const team = {
      id: "t",
      state: "running",
      stop_reason: null,
      lead: { id: "l", read_seq: 0, unread: 0 },
      started_at: "2026-10-17T12:00:00Z",
      deadline_at: null,
      budget: { tokens: 1, spent: 0 },
      waiting_for_you: null,
      version: 1,
      helpers: [
        {
          conversation: "h",
          name: "builder",
          role: "builder",
          workspace: "my_files",
          state: "queued",
          tokens: 0,
        },
      ],
    } as unknown as Team;
    expect(teamView(team).helpers[0]?.workspace).toBe("my_files");
  });
});
