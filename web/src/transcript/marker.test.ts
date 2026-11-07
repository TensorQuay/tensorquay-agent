import { describe, expect, it } from "vitest";

import { unmarked } from "./marker";

const ID = "c0fa72a57fe9b250";
const OPEN = `<untrusted-content source="screen" snapshot="1" kind="dom" id="${ID}">`;
const CLOSE = `</untrusted-content id="${ID}">`;

// WEB-U-363 (the 16e first hour, v01 and v02): the marker that tells the model a tool's text came from outside the
// conversation (tqa-tools untrusted.rs) is the model's, never the person's: a card shows the text inside it, without
// the tag lines.
describe("WEB-U-363 a tool's text without the model's marker lines", () => {
  it("drops a block's opening and closing lines and keeps what's inside, and around it", () => {
    const text = [
      "Snapshot 1 of the focused window (the page's structure, 2 elements).",
      OPEN,
      "Window: Notes",
      'e1 heading "Notes"',
      'e2 button "Mark as read"',
      CLOSE,
    ].join("\n");
    expect(unmarked(text)).toBe(
      [
        "Snapshot 1 of the focused window (the page's structure, 2 elements).",
        "Window: Notes",
        'e1 heading "Notes"',
        'e2 button "Mark as read"',
      ].join("\n"),
    );
  });

  it("treats a person's block the same way", () => {
    const id = "0123456789abcdef";
    expect(
      unmarked(
        `<signed-in-person source="person" by="Ana" id="${id}">\nUse sources.\n</signed-in-person id="${id}">`,
      ),
    ).toBe("Use sources.");
  });

  it("keeps a closing tag with another id and an opening tag inside a block, as the block's own text", () => {
    const inside = `</untrusted-content id="ffffffffffffffff">\n<untrusted-content source="web" id="aaaaaaaaaaaaaaaa">`;
    expect(unmarked(`${OPEN}\n${inside}\n${CLOSE}`)).toBe(inside);
  });

  it("leaves text without a block, and a block that never closes, as they are", () => {
    expect(unmarked("plain output\nline 2")).toBe("plain output\nline 2");
    const unclosed = `${OPEN}\nWindow: Notes`;
    expect(unmarked(unclosed)).toBe(unclosed);
  });

  it("drops a closing line left alone at the start, cut from its opening by a bounded output", () => {
    expect(unmarked(`e9 link "More"\n${CLOSE}`)).toBe('e9 link "More"');
  });
});
