import { describe, expect, it } from "vitest";

import { defaultRoute, modelLabels } from "./model-labels";
import type { RoutedModel } from "./model-line";

const route = (
  name: string,
  extra: Partial<RoutedModel> = {},
): RoutedModel => ({
  name,
  model: name,
  protocol: "openai_chat",
  display_name: null,
  description: null,
  verified: null,
  context_window: null,
  vision: false,
  native_tools: true,
  reasoning: null,
  runs_on: { where: "hosted", provider: null },
  ...extra,
});

describe("WEB-U-321 routes that would read the same are told apart", () => {
  it("leaves a label no other route has as it is", () => {
    const labels = modelLabels([
      route("a", { display_name: "Qwen" }),
      route("b", { model: "deepseek-flash" }),
    ]);
    expect([...labels]).toEqual([
      ["a", "Qwen"],
      ["b", "Deepseek flash"],
    ]);
  });

  it("adds the model's readable name when that differs, ignoring case in the display names", () => {
    const labels = modelLabels([
      route("a", { display_name: "Qwen", model: "qwen3-4b" }),
      route("b", { display_name: "qwen", model: "qwen3-8b" }),
    ]);
    expect(labels.get("a")).toBe("Qwen · Qwen3 4b");
    expect(labels.get("b")).toBe("qwen · Qwen3 8b");
  });

  it("adds where it runs when the models match", () => {
    const labels = modelLabels([
      route("a", {
        display_name: "Qwen",
        model: "qwen3",
        runs_on: { where: "this_computer", provider: null },
      }),
      route("b", {
        display_name: "Qwen",
        model: "qwen3",
        runs_on: { where: "network", provider: null },
      }),
    ]);
    expect(labels.get("a")).toBe("Qwen · On this computer");
    expect(labels.get("b")).toBe("Qwen · On your network");
  });

  it("numbers them in list order when nothing else tells them apart, never naming the route's id", () => {
    const labels = modelLabels([
      route("route-one", { display_name: "Qwen", model: "qwen3" }),
      route("route-two", { display_name: "Qwen", model: "qwen3" }),
    ]);
    expect(labels.get("route-one")).toBe("Qwen (1)");
    expect(labels.get("route-two")).toBe("Qwen (2)");
    expect([...labels.values()].join(" ")).not.toContain("route-");
  });
});

describe("WEB-U-322 New task's default skips a route whose check failed", () => {
  it("takes the first route whose check didn't fail, an unchecked one included", () => {
    expect(
      defaultRoute([
        route("a", { verified: false }),
        route("b", { verified: null }),
        route("c", { verified: true }),
      ]),
    ).toBe("b");
  });

  it("takes the first route when every check failed, and nothing when there is no route", () => {
    expect(
      defaultRoute([
        route("a", { verified: false }),
        route("b", { verified: false }),
      ]),
    ).toBe("a");
    expect(defaultRoute([])).toBe("");
    expect(defaultRoute(undefined)).toBe("");
  });
});
