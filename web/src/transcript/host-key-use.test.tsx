import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { parseFrame } from "../protocol/validate";
import { frame, runId } from "../test/frames";

import { apply, applyAt, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";
import type { Transcript } from "./types";

/** People as the gateway names them: a frame's sender is a user id. */
const ME = "7d1a6b0e-3c2f-4a5b-9c8d-0e1f2a3b4c5d";
const OTHER = "0f9e8d7c-6b5a-4e3d-8c2b-1a0f9e8d7c6b";

const used = {
  type: "host_key.used",
  run_id: runId,
  step: 2,
  /** Whose message started the run: its keys are theirs (ADR-0037 §2). */
  sender_id: ME,
  name: "GITHUB_WORK",
  host: "api.github.com",
  port: 443,
};
const refused = (outcome: string, extra: object = {}) => ({
  ...used,
  type: "host_key.refused",
  outcome,
  ...extra,
});

/** A run whose step 1 ran a command, and whose step 2 is running another. */
function working(): Transcript {
  return [
    frame(1, { type: "run.started", run_id: runId }),
    frame(2, { type: "step.started", step: 1, attempt: 1 }),
    frame(3, {
      type: "tool.call",
      call_id: "call-1",
      name: "shell",
      arguments: { command: "ls" },
    }),
    frame(4, { type: "step.started", step: 2, attempt: 1 }, 2),
    frame(
      5,
      {
        type: "tool.call",
        call_id: "call-2",
        name: "shell",
        arguments: { command: "curl https://api.github.com/user" },
      },
      2,
    ),
  ].reduce(apply, emptyTranscript());
}

const live = (state: Transcript, ...frames: object[]) =>
  frames.reduce(
    (next: Transcript, value) =>
      applyAt(next, parseFrame(JSON.stringify(value)), 1_000),
    state,
  );
const keysOf = (state: Transcript, step: number) => {
  const item = state.items.find(
    (entry) => entry.kind === "step" && entry.step === step,
  );
  return item?.kind === "step" ? (item.keys ?? []) : [];
};

// ADR-0037, provisional until the gateway's openapi lists them: the proxy's view of a keyed call, as unsequenced
// control frames, onto the step running at the time.
describe("WEB-U-147 host key frames onto their step", () => {
  it("reads both frames, strictly", () => {
    expect(parseFrame(JSON.stringify(used))).toEqual(used);
    expect(parseFrame(JSON.stringify(refused("expired")))).toEqual(
      refused("expired"),
    );
    expect(
      parseFrame(JSON.stringify(refused("absent", { name: null }))),
    ).toEqual(refused("absent", { name: null }));
    // To anyone but the sender the gateway sends no name.
    expect(parseFrame(JSON.stringify({ ...used, name: null }))).toEqual({
      ...used,
      name: null,
    });
    expect(
      parseFrame(JSON.stringify(refused("expired", { name: null }))),
    ).toEqual(refused("expired", { name: null }));
    for (const bad of [
      refused("nope"),
      refused("ok"),
      { ...used, sender_id: 7 },
      Object.fromEntries(
        Object.entries(used).filter(([key]) => key !== "sender_id"),
      ),
      { ...used, name: "github_work" },
      { ...used, port: 0 },
      { ...used, port: 65536 },
      { ...used, host: "" },
      { ...used, run_id: 7 },
      { ...used, host: "API.GITHUB.COM" },
      { ...used, host: "api.github.com/acme" },
      { ...used, host: "tqa_ph_0123456789abcdef" },
      { ...used, step: -1 },
      { ...used, step: 1.5 },
    ])
      // A known kind that breaks its schema fails like any other frame; nothing of it reaches the page.
      expect(() => parseFrame(JSON.stringify(bad))).toThrow(
        "Invalid protocol event",
      );
    // The gateway leaves the step out, or null, when none was running.
    const stepless = Object.fromEntries(
      Object.entries(used).filter(([key]) => key !== "step"),
    );
    expect(parseFrame(JSON.stringify(stepless))).toEqual(stepless);
    expect(parseFrame(JSON.stringify({ ...used, step: null }))).toEqual({
      ...used,
      step: null,
    });
  });

  // ADR-0037, Amendment 2: a key may be for an exact IP address, in its one spelling.
  it("reads a key for an exact IP address, refusing it bracketed or in capitals", () => {
    for (const host of ["2001:db8::34", "192.0.2.10", "::ffff:192.0.2.10"])
      expect(parseFrame(JSON.stringify({ ...used, host }))).toEqual({
        ...used,
        host,
      });
    for (const host of ["[2001:db8::34]", "2001:DB8::34"])
      expect(() => parseFrame(JSON.stringify({ ...used, host }))).toThrow(
        "Invalid protocol event",
      );
  });

  it("puts a use without a step on the run's latest step", () => {
    const after = live(
      working(),
      { ...used, step: null },
      {
        ...refused("unavailable"),
        step: undefined,
      },
    );
    expect(keysOf(after, 1)).toEqual([]);
    expect(keysOf(after, 2)).toEqual([
      {
        sender: ME,
        name: "GITHUB_WORK",
        host: "api.github.com",
        port: 443,
        outcome: "ok",
      },
      {
        sender: ME,
        name: "GITHUB_WORK",
        host: "api.github.com",
        port: 443,
        outcome: "unavailable",
      },
    ]);
  });

  it("shows nothing for a use without a step before its run has one", () => {
    const state = [frame(1, { type: "run.started", run_id: runId })].reduce(
      apply,
      emptyTranscript(),
    );
    expect(live(state, { ...used, step: null })).toEqual(state);
  });

  it("puts a use on the step it names, without moving the sequence", () => {
    const before = working();
    const after = live(before, used);
    expect(keysOf(after, 1)).toEqual([]);
    expect(keysOf(after, 2)).toEqual([
      {
        sender: ME,
        name: "GITHUB_WORK",
        host: "api.github.com",
        port: 443,
        outcome: "ok",
      },
    ]);
    expect(after.lastSeq).toBe(before.lastSeq);
  });

  // A use reported after its step ended keeps its own step.
  it("keeps a use on its own step when it arrives after that step ended", () => {
    const state = live(working(), { ...used, step: 1 });
    expect(keysOf(state, 1)).toHaveLength(1);
    expect(keysOf(state, 2)).toEqual([]);
  });

  it("says each key, host and outcome once on a step", () => {
    const state = live(
      working(),
      used,
      used,
      refused("expired"),
      refused("expired"),
      refused("not_allowed"),
    );
    expect(keysOf(state, 2).map((use) => use.outcome)).toEqual([
      "ok",
      "expired",
      "not_allowed",
    ]);
  });

  it("shows nothing for a step not on this page", () => {
    const before = working();
    expect(live(before, { ...used, run_id: "run-other" })).toBe(before);
    expect(live(before, { ...used, step: 9 })).toBe(before);
  });
});

const view = (...frames: object[]) =>
  render(
    <TranscriptView transcript={live(working(), ...frames)} viewer={ME} />,
  );
/**
 * Someone other than the run's sender viewing the task: never the key's name, nor a way to add one. The gateway sends
 * them no name; a name that came anyway is not shown either.
 */
const viewAsOther = (...frames: object[]) =>
  render(
    <TranscriptView transcript={live(working(), ...frames)} viewer={OTHER} />,
  );

// ADR-0037 §8: a keyed call reads as what it did, on the step that made it; a refusal says why, never its code.
describe("WEB-U-148 a task's keyed calls in words", () => {
  it("says which key a call used, after the step's command", () => {
    const { container } = view(used);
    const lines = [...container.querySelectorAll(".activity-lines > li")];
    expect(lines.at(-1)).toHaveTextContent(
      "Called api.github.com with GITHUB_WORK",
    );
  });

  it("brackets an IPv6 address, before its port", () => {
    view(
      { ...used, host: "2001:db8::34" },
      { ...used, host: "fd00::5", port: 8443 },
    );
    expect(
      screen.getByText("Called [2001:db8::34] with GITHUB_WORK"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Called [fd00::5]:8443 with GITHUB_WORK"),
    ).toBeInTheDocument();
  });

  it("shows a port other than 443", () => {
    view({ ...used, host: "git.acme.lan", port: 8443 });
    expect(
      screen.getByText("Called git.acme.lan:8443 with GITHUB_WORK"),
    ).toBeInTheDocument();
  });

  it.each([
    [
      "foreign_run",
      "GITHUB_WORK wasn't sent to api.github.com: it came from an earlier run, and each run has its own.",
    ],
    [
      "foreign_sandbox",
      "GITHUB_WORK wasn't sent to api.github.com: it came from another task.",
    ],
    [
      "expired",
      "GITHUB_WORK wasn't sent to api.github.com: the run had already ended.",
    ],
    [
      "wrong_host",
      "GITHUB_WORK wasn't sent to api.github.com: it's a key for another address.",
    ],
    [
      "wrong_position",
      "GITHUB_WORK wasn't sent to api.github.com: it was put somewhere other than where this key goes.",
    ],
    [
      "not_allowed",
      "GITHUB_WORK wasn't sent to api.github.com: tasks can't reach this address any more.",
    ],
    [
      "revoked",
      "GITHUB_WORK wasn't sent to api.github.com: the key was deleted while the run was using it.",
    ],
    [
      "unrecorded",
      "GITHUB_WORK wasn't sent to api.github.com: its use couldn't be recorded. Try again in a moment.",
    ],
    [
      "unavailable",
      "GITHUB_WORK wasn't sent to api.github.com: the key store was unavailable. Try again in a moment.",
    ],
  ])("words %s", (outcome, words) => {
    const { container } = view(refused(outcome));
    expect(screen.getByText(words)).toBeInTheDocument();
    // Never the code: "unavailable" is also a word of its own sentence, so only the rest of the page is searched.
    expect(container.textContent?.replace(words, "")).not.toContain(outcome);
  });

  it("offers to add a key the task used that isn't set up", () => {
    const { container } = view(
      refused("absent", { name: null, host: "git.acme.lan", port: 8443 }),
    );
    const line = container.querySelector(".activity-lines > li:last-child");
    expect(line).toHaveTextContent(
      "The task used a key for git.acme.lan:8443 that isn't set up.",
    );
    expect(
      within(line as HTMLElement).getByRole("link", {
        name: "Add a key for git.acme.lan:8443",
      }),
    ).toHaveAttribute("href", "/settings/keys/new?address=git.acme.lan%3A8443");
    expect(container).not.toHaveTextContent("absent");
  });
});

// A key's name, and the way to add one, are its owner's; anyone else who sees the task reads what happened without
// them.
describe("WEB-U-148 keyed calls as someone else sees the task", () => {
  it("says a key was used, without its name", () => {
    const { container } = viewAsOther(used);
    expect(
      screen.getByText("Called api.github.com with a key"),
    ).toBeInTheDocument();
    expect(container).not.toHaveTextContent("GITHUB_WORK");
  });

  it("says why a key wasn't sent, without its name", () => {
    const { container } = viewAsOther(refused("expired"));
    expect(
      screen.getByText(
        "A key wasn't sent to api.github.com: the run had already ended.",
      ),
    ).toBeInTheDocument();
    expect(container).not.toHaveTextContent("GITHUB_WORK");
  });

  it("reads the same when the gateway sends no name, as it does to anyone but the sender", () => {
    viewAsOther({ ...used, name: null }, refused("expired", { name: null }));
    expect(
      screen.getByText("Called api.github.com with a key"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "A key wasn't sent to api.github.com: the run had already ended.",
      ),
    ).toBeInTheDocument();
  });

  it("offers no way to add a key", () => {
    const { container } = viewAsOther(refused("absent", { name: null }));
    expect(
      screen.getByText(
        "The task used a key for api.github.com that isn't set up.",
      ),
    ).toBeInTheDocument();
    expect(within(container).queryByRole("link")).toBeNull();
  });
});

// The plan's guard (ADR-0037 §3): a placeholder is the runtime's, never the page's. A frame carrying one where a host
// or a name goes is not a keyed call, so the page never shows it.
describe("WEB-U-148 never a placeholder on the page", () => {
  it("never takes in a frame that carries one", () => {
    for (const bad of [
      { ...used, host: "tqa_ph_0123456789abcdef" },
      { ...used, name: "tqa_ph_0123456789abcdef" },
      refused("expired", { host: "api.tqa_ph_0123456789abcdef.com" }),
    ])
      expect(() => parseFrame(JSON.stringify(bad))).toThrow(
        "Invalid protocol event",
      );
  });
});
