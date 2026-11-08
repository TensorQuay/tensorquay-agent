import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

/** A turn that writes each file at `paths` (one, several or none), then answers. */
const turn = (
  at: number,
  paths: string | readonly string[] | null,
  prompt = "Do it",
  run = runId,
) => {
  const at_ = (seq: number, event: Parameters<typeof frame>[1]) => ({
    ...frame(seq, event),
    run_id: run,
  });
  const frames = [
    at_(at, { type: "message.user", text: prompt, attachments: [] }),
    at_(at + 1, { type: "run.started", run_id: run }),
  ];
  for (const [index, path] of (paths === null
    ? []
    : typeof paths === "string"
      ? [paths]
      : paths
  ).entries())
    frames.push(
      at_(at + 2, {
        type: "tool.call",
        call_id: `w-${at}-${index}`,
        name: "write_file",
        arguments: { path, content: "x" },
      }),
      at_(at + 3, {
        type: "tool.result",
        call_id: `w-${at}-${index}`,
        status: "ok",
        content: "created, 1 bytes\nadded: 1 lines\nremoved: 0 lines",
      }),
    );
  frames.push(
    at_(at + 4, { type: "text.delta", text: "Done." }),
    at_(at + 5, { type: "run.completed", run_id: run }),
  );
  return frames;
};
function show(frames: ReturnType<typeof frame>[]) {
  const onOpenChanges = vi.fn();
  const onFollowUp = vi.fn();
  const onOpenFile = vi.fn();
  render(
    <TranscriptView
      // Numbered in order, as the stream delivers them: the reducer ignores a frame after a gap.
      transcript={frames
        .map((value, index) => ({ ...value, seq: index + 1 }))
        .reduce(apply, emptyTranscript())}
      onOpenChanges={onOpenChanges}
      onFollowUp={onFollowUp}
      onOpenFile={onOpenFile}
    />,
  );
  return { onOpenChanges, onFollowUp, onOpenFile };
}

// A finished task says what it wrote (one file by name, with Open; several with Open changes), and offers at most two
// follow-ups, only ones that clearly follow; none when nothing fits. Never "Undo it".
describe("WEB-U-016 a finished task: what it wrote, and what could follow", () => {
  it("D2: names the one file it wrote, with an Open named for it", () => {
    const { onOpenFile } = show(turn(1, "notes/brief.md"));
    expect(document.querySelector(".turn-summary")).toHaveTextContent(
      "Created brief.md",
    );
    expect(screen.queryByRole("button", { name: "Open changes" })).toBeNull();
    // Each turn's Open says which file, so a transcript has no row of identical "Open" buttons.
    const open = screen.getByRole("button", { name: "Open brief.md" });
    expect(open).toHaveTextContent(/^Open$/);
    fireEvent.click(open);
    expect(onOpenFile).toHaveBeenCalledExactlyOnceWith("notes/brief.md");
    expect(screen.queryByRole("group", { name: "What next" })).toBeNull();
  });

  // The demo's second turn (12 Oct 2026): its step said "updated 1 file" while the footer said "Wrote README.md". The
  // footer now says what the step line says: Created for a new file, Updated for an edit or a replaced file.
  it.each([
    [
      "edit_file",
      {
        path: "README.md",
        edits: [{ old_text: "a", new_text: "b", replace_all: null }],
      },
      "edited README.md: 1 edits, +3 -0 lines",
    ],
    [
      "write_file",
      { path: "README.md", content: "x" },
      "replaced, 1 bytes\nadded: 1 lines\nremoved: 1 lines",
    ],
  ] as const)(
    "D2: a file it changed with %s says Updated",
    (name, args, result) => {
      show([
        frame(1, { type: "message.user", text: "Change it", attachments: [] }),
        frame(2, { type: "run.started", run_id: runId }),
        frame(3, { type: "tool.call", call_id: "c-1", name, arguments: args }),
        frame(4, {
          type: "tool.result",
          call_id: "c-1",
          status: "ok",
          content: result,
        }),
        frame(5, { type: "text.delta", text: "Done." }),
        frame(6, { type: "run.completed", run_id: runId }),
      ]);
      expect(document.querySelector(".turn-summary")).toHaveTextContent(
        /^Updated README\.md$/,
      );
      expect(
        screen.getByRole("button", { name: "Open README.md" }),
      ).toBeVisible();
    },
  );

  // A live check before 17.0: one way to what a run made. Named Open buttons when they cover every file it changed,
  // else one Open changes; never both.
  it("D1: two files it made open by name, and Open changes goes", () => {
    const { onOpenFile } = show(turn(1, ["out/chart.png", "brief.md"]));
    expect(document.querySelector(".turn-summary")).toHaveTextContent(
      "2 files changed",
    );
    expect(screen.queryByRole("button", { name: "Open changes" })).toBeNull();
    const follow = screen.getByRole("group", { name: "What next" });
    expect(
      [...follow.querySelectorAll("button")].map(
        (button) => button.textContent,
      ),
    ).toEqual(["Open the chart", "Open brief.md"]);
    fireEvent.click(screen.getByRole("button", { name: "Open the chart" }));
    expect(onOpenFile).toHaveBeenCalledExactlyOnceWith("out/chart.png");
  });

  it("L8: more files than two named buttons can cover: one Open changes, no names", () => {
    const { onOpenChanges } = show(
      turn(1, ["data/clean.csv", "out/chart.png", "brief.md"]),
    );
    expect(document.querySelector(".turn-summary")).toHaveTextContent(
      "3 files changed",
    );
    fireEvent.click(screen.getByRole("button", { name: "Open changes" }));
    expect(onOpenChanges).toHaveBeenCalledOnce();
    expect(screen.queryByRole("group", { name: "What next" })).toBeNull();
  });

  it("L8: a file the panel can't show isn't covered by a name: one Open changes", () => {
    show(turn(1, ["report.docx", "data/clean.csv"]));
    expect(screen.getByRole("button", { name: "Open changes" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Open clean.csv" })).toBeNull();
  });

  it("D1: says 'the chart' only of an image", () => {
    show(turn(1, ["flowchart-notes.md", "a.txt"]));
    expect(
      screen.getByRole("button", { name: "Open flowchart-notes.md" }),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Open the chart" })).toBeNull();
  });

  it("D1: names each chart when there are two", () => {
    show(turn(1, ["out/sales-chart.png", "out/cost-chart.png"]));
    expect(screen.queryByRole("button", { name: "Open changes" })).toBeNull();
    expect(
      screen.getByRole("button", { name: "Open sales-chart.png" }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Open cost-chart.png" }),
    ).toBeVisible();
  });

  it("offers Run all the tests after a test changed, which always keeps its place", () => {
    const { onFollowUp } = show(
      turn(1, ["tests/test_range.py", "out/chart.png", "brief.md"]),
    );
    // One named button can't cover three files, so Open changes stays and the names go.
    const follow = screen.getByRole("group", { name: "What next" });
    expect(
      [...follow.querySelectorAll("button")].map(
        (button) => button.textContent,
      ),
    ).toEqual(["Run all the tests"]);
    expect(screen.getByRole("button", { name: "Open changes" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Run all the tests" }));
    expect(onFollowUp).toHaveBeenCalledWith("Run all the tests");
    expect(screen.queryByText(/Undo/)).toBeNull();
  });

  it("offers no follow-up when nothing clearly fits", () => {
    show(turn(1, ["README", "report.docx"]));
    expect(screen.getByRole("button", { name: "Open changes" })).toBeVisible();
    expect(screen.queryByRole("group", { name: "What next" })).toBeNull();
  });

  // A general-purpose agent. A document or data file in a spec/ or tests/ folder is not a test.
  it.each(["spec/requirements.md", "tests/plan.txt", "test_results.csv"])(
    "offers no Run all the tests for %s",
    (path) => {
      show(turn(1, [path, "other.bin"]));
      expect(
        screen.queryByRole("button", { name: "Run all the tests" }),
      ).toBeNull();
    },
  );

  it("offers neither when nothing changed", () => {
    show(turn(1, null));
    expect(screen.queryByRole("button", { name: "Open changes" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Open" })).toBeNull();
    expect(screen.queryByRole("group", { name: "What next" })).toBeNull();
  });

  it("keeps follow-ups to the latest turn", () => {
    show([
      ...turn(1, ["tests/test_a.py", "a.py"]),
      ...turn(10, "README.md", "Again", "run-second"),
    ]);
    expect(
      screen.queryByRole("button", { name: "Run all the tests" }),
    ).toBeNull();
  });
});

// A review before 17.0 (WEB-U-016): a stopped turn says "Stopped." and nothing more; "run" is jargon, and the composer
// already offers the next message.
describe("WEB-U-016 a stopped turn", () => {
  it("says only Stopped.", () => {
    show([
      frame(1, { type: "message.user", text: "Do it", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      frame(3, { type: "run.cancelled", run_id: runId }),
    ]);
    expect(document.querySelector(".turn-outcome")).toHaveTextContent(
      /^Stopped\.$/,
    );
  });
});

// WEB-U-083 (the owner's screenshot): the agent named its file by the sandbox's absolute path, and Open went to
// "Workspace / / workspace / index.html", which wasn't there. A tool's path is read as a workspace path first.
describe("WEB-U-083 files a finished task names by their absolute path", () => {
  it("opens the workspace's own file for /workspace/…", () => {
    const { onOpenFile } = show(turn(1, "/workspace/index.html"));
    expect(document.querySelector(".turn-summary")).toHaveTextContent(
      "Created index.html",
    );
    fireEvent.click(screen.getByRole("button", { name: "Open index.html" }));
    expect(onOpenFile).toHaveBeenCalledExactlyOnceWith("index.html");
  });

  it("counts one file named two ways once", () => {
    const { onOpenFile } = show(
      turn(1, ["/workspace/notes/brief.md", "./notes//brief.md"]),
    );
    expect(document.querySelector(".turn-summary")).toHaveTextContent(
      "Created brief.md",
    );
    fireEvent.click(screen.getByRole("button", { name: "Open brief.md" }));
    expect(onOpenFile).toHaveBeenCalledExactlyOnceWith("notes/brief.md");
  });

  // Two files, both offered by name (the rule: named Opens only when they cover every file the run changed).
  it("opens a chart it named absolutely by its workspace path", () => {
    const { onOpenFile } = show(
      turn(1, ["/workspace/out/chart.png", "brief.md"]),
    );
    fireEvent.click(screen.getByRole("button", { name: "Open the chart" }));
    expect(onOpenFile).toHaveBeenCalledExactlyOnceWith("out/chart.png");
  });

  it("offers nothing to open for a file outside the workspace", () => {
    const { onOpenFile } = show(turn(1, "/etc/motd"));
    expect(screen.queryByRole("button", { name: /^Open/ })).toBeNull();
    expect(onOpenFile).not.toHaveBeenCalled();
  });
});
