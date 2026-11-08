import { describe, expect, it } from "vitest";

import { parseTable } from "./csv";

// A CSV or TSV file shows as a table; the parser takes quotes, doubled quotes and line breaks in quotes.
describe("WEB-U-022 reading a file as a table", () => {
  it("splits rows and fields, keeping quoted commas, doubled quotes and line breaks", () => {
    expect(
      parseTable(
        'region,total,note\r\nNorth,1204,"big, early"\nSouth,98,"said ""hi"""\nEast,7,"two\nlines"\n',
        ",",
      ),
    ).toEqual([
      ["region", "total", "note"],
      ["North", "1204", "big, early"],
      ["South", "98", 'said "hi"'],
      ["East", "7", "two\nlines"],
    ]);
  });

  it("reads tab-separated values and keeps empty fields", () => {
    expect(parseTable("a\tb\tc\n1\t\t3", "\t")).toEqual([
      ["a", "b", "c"],
      ["1", "", "3"],
    ]);
  });

  it("drops a last row cut short by a truncated preview", () => {
    expect(parseTable('a,b\n1,2\n3,"unfinish', ",", true)).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
  });

  it("refuses text that isn't a table", () => {
    expect(parseTable('a,b\n1,"never closed\n2,3', ",")).toBeNull();
    expect(parseTable("", ",")).toBeNull();
  });
});
