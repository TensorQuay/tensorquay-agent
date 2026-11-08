import { describe, expect, it } from "vitest";

import { namePasted, pastedFiles } from "./use-paste";

// WEB-U-427: what a paste carries, as the browsers hand it over.
describe("WEB-U-427 the files a paste carries", () => {
  it("reads nothing from a paste without clipboard data", () => {
    expect(pastedFiles(null)).toEqual([]);
  });

  it("falls back to the file list where the clipboard has no items", () => {
    const file = new File(["a"], "notes.txt", { type: "text/plain" });
    const data = { items: undefined, files: [file] } as unknown as DataTransfer;
    expect(pastedFiles(data)).toEqual([file]);
  });

  it("skips a text item among files", () => {
    const file = new File(["a"], "notes.txt", { type: "text/plain" });
    const data = {
      items: [
        { kind: "string", type: "text/plain", getAsFile: () => null },
        { kind: "file", type: "text/plain", getAsFile: () => file },
        { kind: "file", type: "text/plain", getAsFile: () => null },
      ],
      files: [],
    } as unknown as DataTransfer;
    expect(pastedFiles(data)).toEqual([file]);
  });

  it("names only an image with a generic name, by its type", () => {
    let serial = 0;
    const next = () => ++serial;
    const named = (name: string, type: string) =>
      namePasted(new File(["a"], name, { type }), next).name;
    expect(named("image.png", "image/png")).toBe("pasted-image-1.png");
    expect(named("", "image/webp")).toBe("pasted-image-2.webp");
    expect(named("Image.JPG", "image/jpeg")).toBe("pasted-image-3.jpg");
    expect(named("holiday.jpg", "image/jpeg")).toBe("holiday.jpg");
    expect(named("image.png", "image/tiff")).toBe("image.png");
    expect(named("image.txt", "text/plain")).toBe("image.txt");
  });
});
