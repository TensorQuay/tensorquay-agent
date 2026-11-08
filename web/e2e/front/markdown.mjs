import { frame, transcriptFrames } from "./fixtures.mjs";

const section = (index) =>
  `## Section ${index}\n\nA streamed answer with **strong text**, *emphasis* and a [reference](https://example.invalid).\n\n- First item\n- Second item\n\n| File | State |\n| --- | --- |\n| README | Ready |\n\n\`\`\`ts\nconst result = "${"wide".repeat(80)}";\n\`\`\`\n\n`;
export const markdownAnswer = `# Workspace report\n\n${Array.from({ length: 40 }, (_, index) => section(index + 1)).join("")}End of streamed answer.`;
const chunks = markdownAnswer.match(/[\s\S]{1,160}/g) ?? [];
const events = transcriptFrames
  .filter((value) => value.type === "event")
  .slice(0, 11)
  .map((value) => {
    if (value.event.type === "text.delta")
      return { type: "text.delta", text: "" };
    if (value.event.type === "tool.output")
      return { ...value.event, data: "wide-output-".repeat(400) };
    return value.event;
  });
events.push({ type: "step.completed", step: 1, duration_ms: 100 });
events.push({ type: "step.started", step: 2 });
events.push(...chunks.map((text) => ({ type: "text.delta", text })));
events.push({ type: "step.completed", step: 2, duration_ms: 4000 });
events.push({ type: "run.completed", run_id: "run-example" });
let step = 1;
export const markdownFrames = events.map((event, index) => {
  if (event.type === "step.started") step = event.step;
  return frame(index + 1, event, step);
});

// A final answer with a files table (short path and size columns beside a long prose column) and a table
// too wide for the transcript column, which must scroll inside its own container.
const wideColumns = Array.from(
  { length: 12 },
  (_, index) => `Column${index + 1}`,
);
export const tableAnswer = [
  "Files created",
  "",
  "| File | Size | What it contains |",
  "| --- | --- | --- |",
  "| stats/stats.py | 2,124 B | `mean()`, `median()`, `stdev()` (sample standard deviation) plus a shared `_validate()` helper; raises `ValueError` on empty input and `TypeError` on non-numeric input. |",
  "| stats/test_stats.py | 1,770 B | 19 test cases across `TestMean`, `TestMedian` and `TestStdev`, with parametrised empty-input and non-numeric-input tests and cross-checks against the standard library. |",
  "| stats/README.md | 1,122 B | Short documentation: file table, requirements, usage examples, documented behaviour and errors, and how to run the tests. |",
  "",
  "Measurements",
  "",
  `| ${wideColumns.join(" | ")} |`,
  `| ${wideColumns.map(() => "---").join(" | ")} |`,
  `| ${wideColumns.map((name) => `${name.toLowerCase()}-measurement`).join(" | ")} |`,
].join("\n");
