import { describe, expect, it } from "vitest";

import { modelName } from "./model-name";

describe("a model's name as people read it (§10)", () => {
  it("uses the display name, else a readable form of the model's name", () => {
    expect(
      modelName({ display_name: "DeepSeek Flash", model: "deepseek-flash" }),
    ).toBe("DeepSeek Flash");
    expect(modelName({ display_name: null, model: "deepseek-flash" })).toBe(
      "Deepseek flash",
    );
    expect(modelName({ model: "qwen3.8-27b" })).toBe("Qwen3.8 27b");
    expect(modelName({ model: "local_model" })).toBe("Local model");
  });
});
