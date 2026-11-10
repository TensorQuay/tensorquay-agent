import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { frame, transcriptFrames } from "../test/frames";
import { settleLazy } from "../test/lazy";
import { messagesFrames } from "../test/messages";

import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

describe("WEB-U-001 transcript presentation", () => {
  it("renders the same text, reasoning and tools from live delivery and full replay without live token regions", async () => {
    const state = transcriptFrames.reduce(apply, emptyTranscript());
    const { container, rerender } = render(
      <TranscriptView transcript={state} />,
    );
    expect(screen.getByText("Summarize the workspace.")).toBeVisible();
    await settleLazy();
    expect(
      screen.getByText(/The workspace contains a Rust project/),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: /Worked for/ }));
    expect(
      screen.getByText(/^Thought for /).closest("details"),
    ).not.toHaveAttribute("open");
    // The latest tool's card opens with its group.
    expect(
      screen.getByRole("button", { name: "Ran a command: ls" }),
    ).toHaveAttribute("aria-expanded", "true");
    await settleLazy();
    expect(
      screen.getByRole("article", { name: "shell tool call" }),
    ).toBeVisible();
    expect(
      container.querySelector("[aria-live], [role=log], [role=status]"),
    ).toBeNull();
    const before = container.textContent;
    rerender(
      <TranscriptView
        transcript={transcriptFrames.reduce(apply, emptyTranscript())}
      />,
    );
    expect(container.textContent).toBe(before);
  });
});

// WEB-U-222 (ADR-0034 S6): time-placed lines (a team's log) sit among the turns by time, each run of them one list
// read as one log, never spaced as turns are.
describe("WEB-U-222 lines among the turns", () => {
  it("puts earlier lines before the turn in one list, and later ones after it in another", () => {
    const state = transcriptFrames.reduce(apply, emptyTranscript());
    const line = (key: string, at: string) => ({
      key,
      at,
      node: <p>{key}</p>,
    });
    render(
      <TranscriptView
        transcript={state}
        lines={[
          line("first", "0000-01-01T00:00:00Z"),
          line("second", "0000-01-01T00:00:01Z"),
          line("last", "9999-01-01T00:00:00Z"),
        ]}
      />,
    );
    const [early, late] = screen.getAllByRole("list") as [
      HTMLElement,
      HTMLElement,
    ];
    expect(
      within(early)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["first", "second"]);
    expect(within(late).getByRole("listitem")).toHaveTextContent("last");
    const [turn] = screen.getAllByRole("article", { name: "Turn" }) as [
      HTMLElement,
    ];
    expect(
      early.compareDocumentPosition(turn) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      late.compareDocumentPosition(turn) & Node.DOCUMENT_POSITION_PRECEDING,
    ).toBeTruthy();
  });
});

describe("WEB-U-020 attachments in the transcript", () => {
  it("shows each attached file by name under the message, loaded with the first message that has files", async () => {
    const state = apply(
      emptyTranscript(),
      frame(1, {
        type: "message.user",
        text: "Compare these",
        attachments: ["uploads/a.csv", "uploads/Q3_report-2.pdf"],
      }),
    );
    render(<TranscriptView transcript={state} />);
    const message = screen.getByRole("article", { name: "Your message" });
    const files = await within(message).findByRole("list", {
      name: "Attached files",
    });
    expect(
      within(files)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["a.csv", "Q3_report-2.pdf"]);
  });
});

describe("WEB-U-464 a message whose files' list can't load", () => {
  it("keeps the message, the transcript and its files' names (a tab older than an upgrade)", async () => {
    vi.resetModules();
    vi.doMock("../files/SentAttachments", () => {
      throw new Error("Failed to fetch dynamically imported module");
    });
    try {
      const { TranscriptView: Fresh } = await import("./TranscriptView");
      const state = apply(
        emptyTranscript(),
        frame(1, {
          type: "message.user",
          text: "Compare these",
          attachments: ["uploads/a.csv"],
        }),
      );
      render(<Fresh transcript={state} />);
      await settleLazy();
      expect(
        screen.getByRole("article", { name: "Your message" }),
      ).toHaveTextContent("Compare these");
      // Its files are still named, as plain words.
      expect(
        screen.getByRole("article", { name: "Your message" }),
      ).toHaveTextContent("a.csv");
    } finally {
      vi.doUnmock("../files/SentAttachments");
      vi.resetModules();
    }
  });
});

// A Messages run records a reasoning.state before each step's tool calls. Replayed, every tool card still
// opens and nothing of the state shows (WEB-U-037).
describe("WEB-U-037 a replayed Messages run", () => {
  it("opens every tool card after reasoning state", async () => {
    render(
      <TranscriptView
        transcript={messagesFrames.reduce(apply, emptyTranscript())}
      />,
    );
    await settleLazy();
    fireEvent.click(screen.getByRole("button", { name: /Worked for/ }));
    fireEvent.click(
      screen.getByRole("button", { name: /^Created hello\/README\.md/ }),
    );
    // The latest card is already open; opening it again would close it.
    const latest = screen.getByRole("button", {
      name: /^Ran a command: cat hello\/README\.md/,
    });
    if (latest.getAttribute("aria-expanded") !== "true")
      fireEvent.click(latest);
    await settleLazy();
    expect(screen.queryAllByLabelText("Loading tool details")).toEqual([]);
    expect(
      screen.getByRole("article", { name: "write_file tool call" }),
    ).toBeVisible();
    // A command that succeeded says nothing of its exit code.
    expect(
      within(
        screen.getByRole("article", { name: "shell tool call" }),
      ).queryByText(/code 0/),
    ).toBeNull();
    expect(document.body.textContent).not.toMatch(/reasoning\.state|96 B/);
  });
});

// A push that failed for lack of a credential (or a refused one) points to the person's keys. The Product
// Owner's direction: it asks for the key for the site, in plain words, never a "Git credential".
describe("WEB-U-038 push credential hint", () => {
  const push = (content: string, remote?: string) =>
    [
      frame(1, { type: "step.started", step: 1, attempt: 1 }),
      frame(2, {
        type: "tool.call",
        call_id: "call-push",
        name: "git_push",
        arguments: { branch: "feature/readme", ...(remote ? { remote } : {}) },
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
  it.each([
    "git_push_credential_required: Add a Git credential in settings, then try again.",
    "git_push_authentication: The repository refused the stored credential.",
  ])("links a failed push to the key for its site (%s)", (content) => {
    render(
      <TranscriptView
        transcript={push(content, "https://git.example.invalid/team/demo.git")}
      />,
    );
    expect(
      screen.getByRole("link", { name: "Add the key for git.example.invalid" }),
    ).toHaveAttribute("href", "/settings/keys");
    // The line and its link are plain; the gateway's own words stay in the card's collapsed Result until it sends a
    // plain outcome.
    const line = screen.getByRole("button", {
      name: /^Couldn't save the changes/,
    });
    expect(line).toHaveTextContent(
      "Couldn't save the changes to git.example.invalid/team/demo",
    );
    expect(line).not.toHaveTextContent(/Git|push|branch|commit/);
  });

  // PRD §12 item 22: a past push that needed a key points to Keys, naming no project.
  it("points to Keys when a past push named no site", () => {
    render(
      <TranscriptView
        transcript={push("git_push_credential_required: No credential.")}
      />,
    );
    expect(
      screen.getByRole("link", { name: "Add a key in Settings › Keys" }),
    ).toBeVisible();
  });

  it("adds no link for a push refused for another reason", () => {
    render(
      <TranscriptView
        transcript={push(
          "git_push_protected_branch: This branch is protected. Choose another branch.",
        )}
      />,
    );
    expect(screen.queryByRole("link", { name: /^Add the key/ })).toBeNull();
  });
});

// WEB-U-044: a connector call refused for lack of a person's credential points to its page; other refusals read as a
// sentence (WEB-U-016); the tool's own error adds nothing. WEB-U-045, the approval card, was superseded by ADR-0035.
describe("WEB-U-044 connector refusal hints", () => {
  const connector = (content: string) =>
    [
      frame(1, { type: "step.started", step: 1, attempt: 1 }),
      frame(2, {
        type: "tool.call",
        call_id: "call-c",
        name: "mcp.tracker.issues.create",
        arguments: { title: "Crash" },
      }),
      frame(3, { type: "tool.start", call_id: "call-c" }),
      frame(4, { type: "tool.end", call_id: "call-c", status: "error" }),
      frame(5, {
        type: "tool.result",
        call_id: "call-c",
        status: "error",
        content,
      }),
    ].reduce(apply, emptyTranscript());

  it("links a missing credential to the person's connector credentials", () => {
    render(
      <TranscriptView
        transcript={connector(
          "connector_credential_missing: No credential for tracker.",
        )}
      />,
    );
    expect(
      screen.getByRole("link", { name: "Add your credential for tracker" }),
    ).toHaveAttribute("href", "/settings/keys");
  });

  it("says a refused credential was the person's own, and links to check or replace it", () => {
    render(
      <TranscriptView
        transcript={connector(
          "connector_credential_rejected: the gateway's MCP proxy answered 424",
        )}
      />,
    );
    expect(
      screen.getByRole("link", {
        name: "tracker refused your credential: check it or replace it",
      }),
    ).toHaveAttribute("href", "/settings/keys");
    expect(screen.queryByText(/could not be reached/)).toBeNull();
  });

  it("says why other refusals happened, and nothing for the tool's own error", () => {
    const { unmount } = render(
      <TranscriptView
        transcript={connector("connector_timeout: tracker did not answer.")}
      />,
    );
    expect(screen.getByText("tracker did not answer in time")).toBeVisible();
    unmount();
    render(
      <TranscriptView
        transcript={connector("Issue tracker says: title is required.")}
      />,
    );
    expect(
      screen.queryByText(/tracker (did not|could not|is busy)/),
    ).toBeNull();
    expect(
      screen.queryByRole("link", { name: /Add your credential/ }),
    ).toBeNull();
  });

  it("keeps a -32602 answer as the tool's own, with no sentence of the page's", () => {
    render(
      <TranscriptView
        transcript={connector(
          "mcp_invalid_params: Invalid arguments for issues.create",
        )}
      />,
    );
    expect(
      screen.queryByText(/tracker (does not offer|did not|could not|is busy)/),
    ).toBeNull();
    expect(
      screen.queryByRole("link", { name: /Add your credential/ }),
    ).toBeNull();
  });
});

// WEB-E2E-029 on the real stack found it: a connector call the gateway refused before it ran showed its raw error
// ("key_use_unrecorded: the gateway's MCP proxy answered 503") in the open card, beside the step's plain words. A
// failure the step already words keeps its raw result in the card's closed Result, as a push does (WEB-U-016).
describe("WEB-U-016 a worded refusal keeps its raw result folded", () => {
  it("shows the words, and the raw line only in the closed Result", () => {
    const transcript = [
      frame(1, { type: "step.started", step: 1, attempt: 1 }),
      frame(2, {
        type: "tool.call",
        call_id: "call-issue",
        name: "mcp.tracker.issues.create",
        arguments: { title: "Crash" },
      }),
      frame(3, {
        type: "tool.result",
        call_id: "call-issue",
        status: "error",
        content: "key_use_unrecorded: the gateway's MCP proxy answered 503",
      }),
    ].reduce(apply, emptyTranscript());
    render(<TranscriptView transcript={transcript} />);
    expect(
      screen.getByText(
        "The key could not be used because its use could not be recorded. Try again in a moment; if it keeps happening, tell an administrator.",
      ),
    ).toBeVisible();
    const raw = screen.queryByText(/key_use_unrecorded:/);
    if (raw) expect(raw).not.toBeVisible();
  });
});

// Regression (TEST-PLAN §19) of WEB-U-016, from the stack's web_key_use driver (13h-pre): the runtime streams a failed
// call's text as its output before its result (tool.output, tool.end, tool.result), and the open card's Output showed
// the raw line WEB-U-016 had folded only from the result. The events here are the runtime's own order.
describe("WEB-U-186 a worded failure's streamed output stays folded too", () => {
  it("shows the words, and the raw line only in the closed Result, as the runtime sends it", () => {
    const line = "key_use_unrecorded: the gateway's MCP proxy answered 503";
    const transcript = [
      frame(1, { type: "step.started", step: 1, attempt: 1 }),
      frame(2, {
        type: "tool.call",
        call_id: "call-issue",
        name: "mcp.tracker.issues.create",
        arguments: { title: "Crash" },
      }),
      frame(3, { type: "tool.start", call_id: "call-issue" }),
      frame(4, {
        type: "tool.output",
        call_id: "call-issue",
        chunk: 0,
        data: line,
        last: true,
      }),
      frame(5, { type: "tool.end", call_id: "call-issue", status: "error" }),
      frame(6, {
        type: "tool.result",
        call_id: "call-issue",
        status: "error",
        content: line,
      }),
    ].reduce(apply, emptyTranscript());
    render(<TranscriptView transcript={transcript} />);
    expect(
      screen.getByText(
        "The key could not be used because its use could not be recorded. Try again in a moment; if it keeps happening, tell an administrator.",
      ),
    ).toBeVisible();
    for (const raw of screen.queryAllByText(/key_use_unrecorded:/))
      expect(raw).not.toBeVisible();
    // Not lost: the card's closed Result still holds it.
    const result = screen.getByText("Result", { selector: "summary" });
    expect(result.closest("details")).not.toHaveAttribute("open");
    expect(result.closest("details")).toHaveTextContent(line);
  });
});
