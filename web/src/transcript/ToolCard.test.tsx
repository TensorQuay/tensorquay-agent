import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeAll, describe, expect, it } from "vitest";

import { loadDesktopWords } from "./tool-summary";
import { ToolCard } from "./ToolCard";
import type { ToolView } from "./types";

// The Desktop tool's words load with the screen area (WEB-U-422); a page asks for them before its steps render.
beforeAll(() => loadDesktopWords());

const tool: ToolView = {
  id: "call-example",
  name: "shell",
  arguments: { command: "ls" },
  status: "running",
  ran: true,
  output: "",
  result: null,
  startedAt: null,
  durationMs: null,
  exitCode: null,
};

describe("WEB-U-001 tool card behavior", () => {
  // The owner's direction (c8): the line names the project; the card, for those who open it, says what the changes
  // were saved as.
  it("says what a saved push's changes were saved as", () => {
    render(
      <ToolCard
        tool={{
          ...tool,
          name: "git_push",
          arguments: {
            remote: "https://example.com/team/report.git",
            branch: "notes-1",
          },
          status: "ok",
          result: '{"status":"pushed","branch":"notes-1"}',
        }}
        initiallyOpen
      />,
    );
    expect(screen.getByText("Saved as notes-1")).toBeVisible();
  });

  // Was WEB-E2E-005 (axe, scrollable-region-focusable; old tasks still show past push calls): a push's one-line JSON scrolls sideways, so every code block in
  // a card is a named region a keyboard can reach, as the transcript's own code blocks are.
  it("makes every code block a named region a keyboard can reach", () => {
    render(
      <ToolCard
        tool={{
          ...tool,
          name: "git_push",
          arguments: { branch: "feature/notes", commit: "0123456789abcdef" },
          status: "ok",
          output: "",
          result: '{"status":"pushed","message":"Branch pushed."}',
        }}
        initiallyOpen
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Details" }));
    const blocks = document.querySelectorAll("pre.code-block");
    expect(blocks.length).toBeGreaterThanOrEqual(3);
    for (const block of blocks) {
      expect(block).toHaveAttribute("role", "group");
      expect(block).toHaveAttribute("tabindex", "0");
      expect(block).toHaveAccessibleName();
    }
    expect(
      screen.getByRole("group", { name: "All arguments" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Result" })).toBeInTheDocument();
  });

  it("keeps durations of at most one second in the hover text", () => {
    const { rerender } = render(
      <ToolCard tool={{ ...tool, status: "ok", durationMs: 350 }} />,
    );
    expect(screen.getByRole("button", { name: /shell/ })).toHaveAttribute(
      "title",
      "350 ms",
    );
    expect(screen.queryByText("350 ms")).toBeNull();
    rerender(<ToolCard tool={{ ...tool, status: "ok", durationMs: 1000 }} />);
    expect(screen.getByRole("button", { name: /shell/ })).toHaveAttribute(
      "title",
      "1 s",
    );
    expect(screen.queryByText("1 s")).toBeNull();
  });
  it("opens running and failed calls, then collapses a successful call with its duration", () => {
    const { rerender } = render(<ToolCard tool={tool} />);
    expect(screen.getByRole("button", { name: /shell/ })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.getByText("ls")).toBeVisible();
    rerender(
      <ToolCard
        tool={{ ...tool, status: "error", exitCode: 1, durationMs: 1250 }}
      />,
    );
    // Finding A5: a non-zero exit reads as not succeeding, its code kept.
    expect(screen.getByText("Didn't succeed (code 1)")).toBeVisible();
    // The shell's 127 is a missing command: said so, with the code kept (the owner's Mac test).
    rerender(
      <ToolCard
        tool={{ ...tool, status: "error", exitCode: 127, durationMs: 1250 }}
      />,
    );
    expect(
      screen.getByText("Didn't succeed (code 127: command not found)"),
    ).toBeVisible();
    expect(screen.getByText("1.25 s")).toBeVisible();
    rerender(
      <ToolCard
        tool={{ ...tool, status: "ok", exitCode: 0, durationMs: 1250 }}
      />,
    );
    expect(screen.getByRole("button", { name: /shell/ })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(screen.getByText("ls")).not.toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: /shell/ }));
    expect(screen.getByText("ls")).toBeVisible();
    // A command that succeeded says nothing of its exit code.
    expect(screen.queryByText(/code 0/)).toBeNull();
  });

  it("places the argument before output and caps output at twenty lines until requested", () => {
    const output = Array.from(
      { length: 25 },
      (_, index) => `output ${index + 1}`,
    ).join("\n");
    // An ended call's card (a running one shows its latest lines as they come, WEB-U-438).
    render(<ToolCard tool={{ ...tool, status: "ok", output }} initiallyOpen />);
    const card = screen.getByRole("article", { name: "shell tool call" });
    const argument = within(card).getByLabelText("What it was given");
    const outputBlock = within(card).getByLabelText("Result");
    expect(argument).toHaveTextContent("ls");
    expect(
      argument.compareDocumentPosition(outputBlock) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(outputBlock).toHaveTextContent("output 20");
    expect(outputBlock).not.toHaveTextContent("output 21");
    fireEvent.click(within(card).getByRole("button", { name: "Show more" }));
    expect(outputBlock).toHaveTextContent("output 25");
    fireEvent.click(within(card).getByRole("button", { name: "Show less" }));
    expect(outputBlock).not.toHaveTextContent("output 21");
  });

  it("uses the file path and shows a never-run result without running state or duration", () => {
    render(
      <ToolCard
        tool={{
          ...tool,
          name: "file_read",
          arguments: { path: "README.md" },
          status: "error",
          ran: false,
          result: "Permission denied",
        }}
      />,
    );
    expect(screen.getByText("README.md")).toBeVisible();
    expect(screen.getByText("Permission denied")).toBeVisible();
    expect(screen.queryByText("Running")).not.toBeInTheDocument();
    expect(screen.queryByText(/ s$| ms$/)).not.toBeInTheDocument();
  });

  it("keeps a result disclosure closed when a call ran but produced no streamed output", () => {
    render(
      <ToolCard
        tool={{ ...tool, status: "error", result: "A detailed result" }}
      />,
    );
    expect(screen.getByText("Result").closest("details")).not.toHaveAttribute(
      "open",
    );
    expect(screen.getByText("A detailed result")).not.toBeVisible();
  });
});

// ADR-0035: nothing waits for approval. A call made but not yet started reads as starting, never as waiting.
describe("a called command that has not started reads as starting (no approvals, ADR 35)", () => {
  it("says Starting…, and never Waiting for approval", () => {
    render(<ToolCard tool={{ ...tool, status: "prepared" }} />);
    expect(screen.getByText("Starting…")).toBeInTheDocument();
    expect(screen.queryByText(/Waiting for approval/)).toBeNull();
  });
});

// The c16 live check (LC-04): a step-in reached the lead as a read whose Output was the raw block for the model.
describe("WEB-U-327 a team read's card", () => {
  const block =
    '<signed-in-person source="person" seq="5" type="person_message" from="your person" id="b332b4cc72cc104b">\n' +
    "Use only facts you can name a source for.\n" +
    '</signed-in-person id="b332b4cc72cc104b">';

  it("says how many messages the lead read, never the blocks or 'your person'", () => {
    render(
      <ToolCard
        tool={{
          ...tool,
          name: "read_reports",
          arguments: {},
          status: "ok",
          output: block,
          result: block,
        }}
        initiallyOpen
      />,
    );
    expect(screen.getByText("Read 1 message from the team.")).toBeVisible();
    const card = screen.getByRole("article");
    expect(card).not.toHaveTextContent("signed-in-person");
    expect(card).not.toHaveTextContent("your person");
    expect(card).not.toHaveTextContent("Use only facts");
    expect(within(card).queryByText("Result")).not.toBeInTheDocument();
  });

  it("says a helper's empty wait found nothing new", () => {
    const words =
      "Nothing yet; end your turn, and you'll be woken by your lead's next message.";
    render(
      <ToolCard
        tool={{
          ...tool,
          name: "wait_for_lead",
          arguments: {},
          status: "ok",
          output: words,
          result: words,
        }}
        initiallyOpen
      />,
    );
    expect(screen.getByText("No new messages.")).toBeVisible();
    expect(screen.getByRole("article")).not.toHaveTextContent("end your turn");
  });

  it("keeps a failed read's card as any failed call's", () => {
    render(
      <ToolCard
        tool={{
          ...tool,
          name: "read_reports",
          arguments: {},
          status: "error",
          output: "",
          result: "the team has ended",
        }}
      />,
    );
    expect(screen.queryByText(/from the team/)).not.toBeInTheDocument();
    expect(screen.getByText("the team has ended")).toBeInTheDocument();
  });
});

// WEB-U-363 (the 16e first hour, v01 and v02): a desktop step's output and a web page's result never show the
// model's marker lines, open or folded; what's inside them stays.
describe("WEB-U-363 a tool card without the model's marker lines", () => {
  const id = "c0fa72a57fe9b250";
  const marked = [
    "Snapshot 1 of the focused window (the page's structure, 1 elements).",
    `<untrusted-content source="screen" snapshot="1" kind="dom" id="${id}">`,
    "Window: Notes",
    'e1 button "Mark as read"',
    `</untrusted-content id="${id}">`,
  ].join("\n");

  it("shows a desktop step's element list without the tags", () => {
    render(
      <ToolCard
        tool={{
          ...tool,
          name: "computer",
          arguments: { action: "snapshot" },
          status: "ok",
          output: marked,
          result: marked,
        }}
        initiallyOpen
      />,
    );
    const shown = document.querySelector(".tool-body")?.textContent ?? "";
    expect(shown).toContain('e1 button "Mark as read"');
    expect(shown).not.toContain("untrusted-content");
  });

  it("shows a fetched page's folded result without the tags", () => {
    render(
      <ToolCard
        tool={{
          ...tool,
          name: "fetch_url",
          arguments: { url: "https://example.com" },
          status: "ok",
          output: "",
          result: marked.replace(
            'source="screen" snapshot="1" kind="dom"',
            'source="web" url="https://example.com"',
          ),
        }}
        initiallyOpen
      />,
    );
    fireEvent.click(screen.getByText("Result"));
    const shown = document.querySelector(".tool-body")?.textContent ?? "";
    expect(shown).toContain("Window: Notes");
    expect(shown).not.toContain("untrusted-content");
  });
});

// WEB-U-368 (the lead's live session on 16e): a desktop step's output read "Typed 17 characters into e8; the screen
// changed." A ref is the model's handle, meaningless to a person: the output names the element as the step's title
// does (its name quoted, else its role in words), and a snapshot's element list after it keeps its refs.
describe("WEB-U-368 a desktop step's output names its element, never its ref", () => {
  const id = "0123456789abcdef";
  const typed = (
    target: { name: string; role: "textbox" | "other" } | undefined,
  ) => (
    <ToolCard
      tool={{
        ...tool,
        name: "computer",
        arguments: { action: "type", ref: "e8", text: "anna@example.com " },
        status: "ok",
        output: [
          "Typed 17 characters into e8; the screen changed.",
          `<untrusted-content source="screen" snapshot="2" kind="dom" id="${id}">`,
          'e8 textbox "Email"',
          `</untrusted-content id="${id}">`,
        ].join("\n"),
        result: null,
        target,
      }}
      initiallyOpen
    />
  );

  it("says the element's name, or its role, where the ref was", () => {
    const { unmount } = render(typed({ name: "Email", role: "textbox" }));
    let shown = document.querySelector(".tool-body pre")?.textContent ?? "";
    expect(shown).toContain(
      "Typed 17 characters into “Email”; the screen changed.",
    );
    expect(shown).toContain('e8 textbox "Email"');
    unmount();
    render(typed({ name: "", role: "textbox" }));
    shown = document.querySelector(".tool-body pre")?.textContent ?? "";
    expect(shown).toContain(
      "Typed 17 characters into a text box; the screen changed.",
    );
  });

  it("says an element when nothing names it", () => {
    render(typed(undefined));
    const shown = document.querySelector(".tool-body pre")?.textContent ?? "";
    expect(shown).toContain(
      "Typed 17 characters into an element; the screen changed.",
    );
    expect(shown.split("\n")[0]).not.toMatch(/\be8\b/);
  });
});
