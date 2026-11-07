import { describe, expect, it } from "vitest";

import { parseScale, parseWorkers, unitCaps } from "../../deadline-scale";

// WEB-U-302: the unit tests' caps scale with the harness's load scale, read as TK-U-012 and WEB-U-189 read it.
describe("WEB-U-302 the unit tests' load scale", () => {
  it.each([
    [undefined, 1],
    ["", 1],
    ["1", 1],
    ["4", 4],
    [" 4 ", 4],
    ["20", 20],
    ["0", 1],
    ["21", 1],
    ["-2", 1],
    ["1.5", 1],
    ["four", 1],
  ])("reads %j as %i", (value, scale) => {
    expect(parseScale(value)).toBe(scale);
  });

  it("keeps the defaults at 1 and scales both caps", () => {
    expect(unitCaps(1)).toEqual({ asyncUtilMs: 1_000, testMs: 5_000 });
    expect(unitCaps(4)).toEqual({ asyncUtilMs: 4_000, testMs: 20_000 });
  });
});

// WEB-U-313: how many workers the unit tests use, `TQA_VITEST_MAX_WORKERS`: an integer from 1 to 8, else the 2 they
// always used. A loaded host's runs set it lower; nothing else changes.
describe("WEB-U-313 the unit tests' worker count", () => {
  it.each([
    [undefined, 2],
    ["", 2],
    ["1", 1],
    ["2", 2],
    [" 3 ", 3],
    ["8", 8],
    ["0", 2],
    ["9", 2],
    ["-1", 2],
    ["1.5", 2],
    ["one", 2],
  ])("reads %j as %i", (value, workers) => {
    expect(parseWorkers(value)).toBe(workers);
  });
});
