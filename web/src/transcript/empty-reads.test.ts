import { describe, expect, it } from "vitest";

import { frame, runId } from "../test/frames";

import { present } from "./present";
import { apply, emptyTranscript } from "./reducer";

const NOTHING = "Nothing new yet.";

/** One team read in its own step: its call, start, end and answer, numbered from `seq` (frames run without gaps). */
const read =
  (step: number, name: string, answer: string, status: "ok" | "error" = "ok") =>
  (seq: number) => {
    const id = `c-${step}`;
    return [
      frame(seq, { type: "step.started", step, attempt: 1 }, step),
      frame(
        seq + 1,
        { type: "tool.call", call_id: id, name, arguments: {} },
        step,
      ),
      frame(seq + 2, { type: "tool.start", call_id: id }, step),
      frame(seq + 3, { type: "tool.end", call_id: id, status }, step),
      frame(
        seq + 4,
        { type: "tool.result", call_id: id, status, content: answer },
        step,
      ),
    ];
  };
const FRAMES_PER_READ = 5;

const work = (...reads: ReturnType<typeof read>[]) => {
  const transcript = [
    frame(1, { type: "message.user", text: "Lead the team.", attachments: [] }),
    frame(2, { type: "run.started", run_id: runId }),
    ...reads.flatMap((each, index) => each(3 + index * FRAMES_PER_READ)),
  ].reduce(apply, emptyTranscript());
  return present(transcript)[0]?.work ?? [];
};

// WEB-U-238 (the 14b page pass, H02): a lead waiting on its helpers reads again and again; reads in a row that found
// nothing are one quiet line, "Checked for reports 3 times · nothing new", never the same empty line three times.
describe("WEB-U-238 empty team reads in a row fold to one line", () => {
  it("folds three empty reads into one line with their count", () => {
    const items = work(
      read(1, "read_reports", NOTHING),
      read(2, "read_reports", NOTHING),
      read(3, "read_reports", NOTHING),
    );
    expect(items).toEqual([
      expect.objectContaining({
        kind: "reads",
        tool: "read_reports",
        count: 3,
      }),
    ]);
  });

  it("keeps a read that found something, and folds only the runs around it", () => {
    const items = work(
      read(1, "read_reports", NOTHING),
      read(2, "read_reports", NOTHING),
      read(
        3,
        "read_reports",
        "<untrusted-content>North: 4.1 M</untrusted-content>",
      ),
      read(4, "read_reports", NOTHING),
    );
    expect(items.map((item) => item.kind)).toEqual(["reads", "tool", "tool"]);
    expect(items[0]).toMatchObject({ count: 2 });
  });

  it("leaves a single empty read, a failed one and a helper's own wait as their own lines", () => {
    expect(work(read(1, "read_reports", NOTHING)).map((i) => i.kind)).toEqual([
      "tool",
    ]);
    expect(
      work(
        read(1, "read_reports", NOTHING, "error"),
        read(2, "read_reports", NOTHING, "error"),
      ).map((i) => i.kind),
    ).toEqual(["tool", "tool"]);
    const helper =
      "Nothing yet; end your turn, and you'll be woken by your lead's next message.";
    expect(
      work(read(1, "wait_for_lead", helper), read(2, "wait_for_lead", helper)),
    ).toEqual([
      expect.objectContaining({
        kind: "reads",
        tool: "wait_for_lead",
        count: 2,
      }),
    ]);
    // Different reads in a row don't fold together.
    expect(
      work(
        read(1, "read_reports", NOTHING),
        read(2, "wait_for_lead", helper),
      ).map((i) => i.kind),
    ).toEqual(["tool", "tool"]);
  });
});
