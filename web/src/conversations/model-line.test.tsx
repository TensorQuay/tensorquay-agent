import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { english } from "../i18n/translate";

import { modelLine, type RoutedModel } from "./model-line";
import { ModelPicker } from "./ModelPicker";

const base: RoutedModel = {
  name: "local",
  display_name: "Local model",
  description: null,
  verified: null,
  answering: null,
  model: "qwen3",
  protocol: "openai_chat",
  context_window: 32768,
  vision: false,
  native_tools: true,
  reasoning: null,
  runs_on: { where: "this_computer", provider: null },
};

// The UI standard's picker: every row has a muted second line or none does. A model's says where it runs, then what
// it can do, in plain words; the administrator's description replaces it; never the context window.
describe("WEB-U-103 a model's second line", () => {
  it.each([
    [
      "a local route that reads images",
      { runs_on: { where: "this_computer", provider: null }, vision: true },
      "On this computer · Reads images",
    ],
    [
      "a hosted route that reads images",
      { runs_on: { where: "hosted", provider: "DeepSeek" }, vision: true },
      "DeepSeek · Reads images",
    ],
    [
      "a hosted route with no provider name, which thinks",
      {
        runs_on: { where: "hosted", provider: null },
        reasoning: { allowed: ["low", "high"], default: "low" },
      },
      // Never a host name: it can name an internal proxy or machine, and every member sees this list.
      "Hosted · Thinks before answering",
    ],
    [
      "a route on the network that knows only where it runs",
      { runs_on: { where: "network", provider: null } },
      "On your network",
    ],
  ] satisfies [string, Partial<RoutedModel>, string][])(
    "reads %s",
    (_, extra, line) => {
      expect(modelLine({ ...base, ...extra }, english)).toBe(line);
    },
  );

  it("always says where it runs, and never names its context window", () => {
    expect(modelLine(base, english)).toBe("On this computer");
    expect(
      modelLine(
        {
          ...base,
          runs_on: { where: "this_computer", provider: null },
          vision: true,
        },
        english,
      ),
    ).not.toMatch(/context|token|long/i);
  });

  it("uses the administrator's description instead, in the picker", () => {
    render(
      <ModelPicker
        route="local"
        onChange={() => undefined}
        models={[
          {
            ...base,
            runs_on: { where: "this_computer", provider: null },
            vision: true,
          },
          {
            ...base,
            name: "hosted",
            display_name: "DeepSeek Flash",
            description: "Fast answers for everyday work",
            runs_on: { where: "hosted", provider: "DeepSeek" },
          },
        ]}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /^Model:/ }));
    expect(
      screen.getByRole("menuitemradio", { name: /^Local model/ }),
    ).toHaveTextContent("On this computer · Reads images");
    // Read as two phrases, not run together ("Local modelOn this computer").
    expect(
      screen.getByRole("menuitemradio", {
        name: "Local model On this computer · Reads images",
      }),
    ).toBeVisible();
    const hosted = screen.getByRole("menuitemradio", {
      name: /^DeepSeek Flash/,
    });
    expect(hosted).toHaveTextContent("Fast answers for everyday work");
    expect(hosted).not.toHaveTextContent("DeepSeek ·");
  });
});

describe("WEB-U-469 the picker says when a model isn't answering", () => {
  it("names it on that model's row only, in plain words", () => {
    render(
      <ModelPicker
        route="cloud"
        onChange={() => undefined}
        models={[
          {
            ...base,
            name: "stopped",
            display_name: "Qwen3.8 27B",
            description: "On the RTX 5090 in this computer",
            verified: true,
            answering: false,
          },
          { ...base, name: "cloud", display_name: "Opus", answering: true },
        ]}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /^Model:/ }));
    const stopped = screen.getByRole("menuitemradio", {
      name: /^Qwen3\.8 27B/,
    });
    expect(stopped).toHaveTextContent("Not answering right now");
    expect(stopped).toHaveTextContent("On the RTX 5090 in this computer");
    expect(
      screen.getByRole("menuitemradio", { name: /^Opus/ }),
    ).not.toHaveTextContent("Not answering");
  });
});
