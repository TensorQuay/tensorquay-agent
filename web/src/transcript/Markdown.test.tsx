import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { frame } from "../test/frames";
import { settleLazy } from "../test/lazy";

import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

// The first render's one-time warm-up (median 330 ms, over 1 s on a busy machine) is awaited, never raced (WEB-U-014).
async function rendered() {
  await settleLazy();
  expect(screen.queryAllByLabelText("Formatting answer")).toHaveLength(0);
}

function answer(text: string) {
  return [
    frame(1, { type: "step.started", step: 1, attempt: 1 }),
    frame(2, { type: "text.delta", text }),
  ].reduce(apply, emptyTranscript());
}
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(document, "execCommand");
});

describe("WEB-U-014 assistant Markdown", () => {
  it.each([
    ["A paragraph", "p", "A paragraph"],
    ["**strong**", "strong", "strong"],
    ["*emphasis*", "em", "emphasis"],
    ["~~removed~~", "del", "removed"],
    ["`inline`", "code", "inline"],
    ["> quotation", "blockquote", "quotation"],
    ["- first\n- second", "ul", "firstsecond"],
    ["1. first\n2. second", "ol", "firstsecond"],
    ["---", "hr", ""],
    ...Array.from({ length: 6 }, (_, index) => [
      `${"#".repeat(index + 1)} Heading`,
      `h${index + 1}`,
      "Heading",
    ]),
  ])("renders %s as %s", async (text, tag, content) => {
    const { container } = render(<TranscriptView transcript={answer(text)} />);
    await rendered();
    expect(container.querySelector(tag)).toBeInTheDocument();
    expect(container.querySelector(tag)?.textContent?.replace(/\n/g, "")).toBe(
      content,
    );
  });

  it("renders task lists, tables and labelled fenced code", async () => {
    render(
      <TranscriptView
        transcript={answer(
          "- [x] Done\n- [ ] Pending\n\n| File | State |\n| --- | --- |\n| README | Ready |\n\n```ts\nconst value = 1;\n```\n",
        )}
      />,
    );
    await rendered();
    expect(screen.getAllByRole("checkbox")[0]).toBeChecked();
    for (const checkbox of screen.getAllByRole("checkbox"))
      expect(checkbox).toBeDisabled();
    expect(screen.getByRole("table")).toHaveTextContent("FileStateREADMEReady");
    expect(screen.getByText("ts", { exact: true })).toBeVisible();
    expect(screen.getByRole("button", { name: "Copy code" })).toBeEnabled();
    expect(screen.getByText("const value = 1;")).toBeVisible();
  });

  // A value such as "2,124 B" stays on one line; prose cells keep wrapping between words.
  it("WEB-U-027 keeps short table cells whole and preserves column alignment", async () => {
    render(
      <TranscriptView
        transcript={answer(
          "| File | Size | What it contains |\n| --- | ---: | --- |\n| stats/stats.py | 2,124 B | Mean, median and standard deviation with shared validation. |\n",
        )}
      />,
    );
    await rendered();
    const short = ["File", "Size", "stats/stats.py", "2,124 B"];
    for (const text of short)
      expect(screen.getByText(text)).toHaveClass("markdown-cell-short");
    expect(screen.getByText(/^Mean, median/)).not.toHaveClass(
      "markdown-cell-short",
    );
    expect(screen.getByText("2,124 B")).toHaveStyle({ textAlign: "right" });
  });

  it("shows raw HTML and remote images as text and restricts link schemes", async () => {
    const raw =
      '<script>alert(1)</script>\n\n<img src="https://example.invalid/pixel">';
    const { container } = render(
      <TranscriptView
        transcript={answer(
          `${raw}\n\n![Diagram](https://example.invalid/diagram.png)\n\n[Unsafe](javascript:alert%281%29) [Relative](/local) [Data](data:text/html,unsafe)\n\n[Web](https://example.invalid) [HTTP](http://example.invalid) [Mail](mailto:team@example.invalid)`,
        )}
      />,
    );
    await rendered();
    expect(container.querySelector("script, img, iframe")).toBeNull();
    expect(container).toHaveTextContent("<script>alert(1)</script>");
    expect(container).toHaveTextContent(
      '<img src="https://example.invalid/pixel">',
    );
    expect(container).toHaveTextContent(
      "Diagram (https://example.invalid/diagram.png)",
    );
    for (const label of ["Unsafe", "Relative", "Data"]) {
      expect(screen.getByText(label)).toBeVisible();
      expect(screen.queryByRole("link", { name: label })).toBeNull();
    }
    expect(screen.getAllByRole("link")).toHaveLength(3);
    for (const link of screen.getAllByRole("link")) {
      expect(link).toHaveAttribute("target", "_blank");
      expect(link).toHaveAttribute("rel", "noopener noreferrer nofollow");
    }
  });

  it("renders every prefix of an incomplete document, then settles when complete", async () => {
    const document =
      "# Answer\n\n- A list\n\n| A | B |\n| - | - |\n| One | Two |\n\n```ts\nconst value = '<img>';\n```";
    const { rerender, container } = render(
      <TranscriptView transcript={answer("")} />,
    );
    for (let end = 0; end <= document.length; end++) {
      expect(() =>
        rerender(
          <TranscriptView transcript={answer(document.slice(0, end))} />,
        ),
      ).not.toThrow();
      await rendered();
      expect(container.querySelector("img, script")).toBeNull();
    }
    expect(screen.getByRole("heading", { name: "Answer" })).toBeVisible();
    expect(container.querySelector("pre code")).toHaveTextContent(
      "const value = '<img>';",
    );
  });

  it.each(["native", "missing", "denied"])(
    "copies literal code with the %s clipboard path",
    async (kind) => {
      const writeText = vi
        .fn()
        .mockImplementation(() =>
          kind === "native"
            ? Promise.resolve()
            : Promise.reject(new Error("Denied")),
        );
      vi.stubGlobal(
        "navigator",
        kind === "missing" ? {} : { clipboard: { writeText } },
      );
      const copy = vi.fn(() => {
        expect(document.querySelector("textarea")?.value).toBe(
          "Copy this exact text.\n",
        );
        return true;
      });
      Object.defineProperty(document, "execCommand", {
        configurable: true,
        value: copy,
      });
      render(
        <TranscriptView
          transcript={answer("```text\nCopy this exact text.\n```")}
        />,
      );
      const button = await screen.findByRole("button", { name: "Copy code" });
      button.focus();
      fireEvent.click(button);
      expect(
        await screen.findByRole("button", { name: "Copied" }),
      ).toBeVisible();
      if (kind === "native") {
        expect(writeText).toHaveBeenCalledExactlyOnceWith(
          "Copy this exact text.\n",
        );
        expect(copy).not.toHaveBeenCalled();
      } else expect(copy).toHaveBeenCalledExactlyOnceWith("copy");
      expect(button).toHaveFocus();
      expect(document.querySelector("textarea")).toBeNull();
    },
  );

  it("reports copy failure inline and removes the temporary textarea", async () => {
    vi.stubGlobal("navigator", {});
    Object.defineProperty(document, "execCommand", {
      configurable: true,
      value: () => false,
    });
    render(
      <TranscriptView transcript={answer("```text\nKeep this code.\n```")} />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Copy code" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Copy failed. Select the code and copy it manually.",
    );
    expect(document.querySelector("textarea")).toBeNull();
  });

  // WEB-U-296 (a first-hour check before 17.0): a model's empty fenced block, its opening fence alone or with only
  // blank lines inside, shows no code box with Copy code; the answer's own text before and after it stays.
  it.each([
    ["an opening fence with nothing after it", "Done.\n\n```json\n"],
    [
      "a block with only blank lines",
      "Done.\n\n```json\n\n  \n```\n\nThat's all.",
    ],
    ["an untyped empty block", "Done.\n\n```\n```\n\nThat's all."],
  ])("WEB-U-296: %s renders no code box", async (_, text) => {
    render(<TranscriptView transcript={answer(text)} />);
    await rendered();
    expect(screen.getByText("Done.")).toBeVisible();
    if (text.includes("That's all."))
      expect(screen.getByText("That's all.")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Copy code" })).toBeNull();
    expect(document.querySelector(".markdown-code")).toBeNull();
  });

  it("WEB-U-296: a block with code in it still has its box", async () => {
    render(<TranscriptView transcript={answer("```json\n{}\n```\n")} />);
    await rendered();
    expect(screen.getByRole("button", { name: "Copy code" })).toBeEnabled();
  });
});
