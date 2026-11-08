import { readFileSync } from "node:fs";
import { join } from "node:path";

import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { catalogue, english, translator } from "../i18n/translate";

import { capabilityNames } from "./capabilities";
import { EnvironmentPicker, type EnvironmentChoice } from "./EnvironmentPicker";

const locale = (code: string) =>
  translator(
    code,
    catalogue(
      JSON.parse(
        readFileSync(join("src", "i18n", "locales", `${code}.json`), "utf8"),
      ),
    ),
  );

const choices: EnvironmentChoice[] = [
  {
    id: "workstation",
    label: "Workstation",
    capabilities: ["shell", "python", "node", "build", "browser"],
    description: "For most tasks: web pages, documents, data and code",
    size: { cpus: 2, memoryMib: 4096 },
  },
  {
    id: "minimal",
    label: "Minimal",
    capabilities: ["shell"],
    description: null,
  },
];

describe("WEB-U-018 environment picker", () => {
  it("names declared capabilities in words and chooses by identity without making a request", () => {
    const onChange = vi.fn();
    render(
      <EnvironmentPicker
        choices={choices}
        value="workstation"
        onChange={onChange}
      />,
    );
    const trigger = screen.getByRole("button", {
      name: "Environment: Workstation",
    });
    // Tools read in words in the menu, not as a row of icons on the button.
    expect(within(trigger).queryByRole("img")).toBeNull();
    fireEvent.click(trigger);
    const menu = screen.getByRole("menu", { name: "Environment" });
    expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(2);
    expect(
      screen.getByRole("menuitemradio", { name: /^Workstation/ }),
    ).toHaveAttribute("aria-checked", "true");
    const minimal = screen.getByRole("menuitemradio", { name: /^Minimal/ });
    expect(within(minimal).getByText("Shell")).toBeVisible();
    // Its size at the end of the line, muted: what people choose by, not processes or disk (the sizes review).
    expect(
      screen.getByRole("menuitemradio", { name: /^Workstation/ }),
    ).toHaveTextContent(
      "For most tasks: web pages, documents, data and code · 2 CPUs · 4 GB",
    );
    // WEB-U-185: the size is written as everywhere else, and never split across lines.
    expect(
      within(
        screen.getByRole("menuitemradio", { name: /^Workstation/ }),
      ).getByText("2 CPUs · 4 GB"),
    ).toHaveClass("nowrap");
    fireEvent.click(minimal);
    expect(onChange).toHaveBeenCalledExactlyOnceWith("minimal");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
  });

  // The c16 UX review's second pass, #29: with a description the capabilities are only a tooltip, so a desktop
  // environment says it has one where it can be read, as its size is.
  it("marks a described desktop environment as a Desktop", () => {
    render(
      <EnvironmentPicker
        choices={[
          ...choices,
          {
            id: "desktop",
            label: "Desktop",
            capabilities: ["shell", "browser", "desktop"],
            description: "A screen the agent works on",
            size: { cpus: 2, memoryMib: 4096 },
          },
        ]}
        value="workstation"
        onChange={vi.fn()}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Environment: Workstation" }),
    );
    expect(
      screen.getByRole("menuitemradio", { name: /^Desktop/ }),
    ).toHaveTextContent(
      "A screen the agent works on · Desktop · 2 CPUs · 4 GB",
    );
    // Others are as they were.
    expect(
      screen.getByRole("menuitemradio", { name: /^Workstation/ }),
    ).toHaveTextContent(
      "For most tasks: web pages, documents, data and code · 2 CPUs · 4 GB",
    );
  });

  // A pass over the pages before 17.0: without a description the line lists the tools, and a desktop was one more of
  // its commas ("Shell, Browser, Desktop · 2 CPUs"); it is marked as a described one is, after the tools.
  it("marks an undescribed desktop environment as a Desktop after its tools", () => {
    render(
      <EnvironmentPicker
        choices={[
          ...choices,
          {
            id: "desktop",
            label: "Desktop",
            capabilities: ["shell", "desktop", "browser"],
            description: null,
            size: { cpus: 2, memoryMib: 4096 },
          },
          {
            id: "screen",
            label: "Screen only",
            capabilities: ["desktop"],
            description: null,
          },
        ]}
        value="workstation"
        onChange={vi.fn()}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Environment: Workstation" }),
    );
    expect(
      screen.getByRole("menuitemradio", { name: /^Desktop/ }),
    ).toHaveTextContent(/^Desktop Shell, Browser · Desktop · 2 CPUs · 4 GB$/);
    expect(
      screen.getByRole("menuitemradio", { name: /^Screen only/ }),
    ).toHaveTextContent(/^Screen only Desktop$/);
  });

  // A pass over the pages before 17.0: the tools were joined with ", " in every language; Chinese and Japanese list
  // with 、 (as limits.ceiling does), and a language without its own separator keeps English's.
  it("joins the tools with the reader's own list separator", () => {
    const tools = ["shell", "python", "build"] as const;
    expect(capabilityNames(tools, english)).toBe("Shell, Python, Build tools");
    expect(capabilityNames(tools, locale("zh-Hans"))).toBe(
      "命令行、Python、构建工具",
    );
    expect(capabilityNames(tools, locale("ja"))).not.toContain(", ");
    expect(capabilityNames(tools, locale("ja"))).toContain("、");
  });

  it("supports arrows, Home, End, typeahead and Escape without committing a cancelled choice", () => {
    const onChange = vi.fn();
    render(
      <EnvironmentPicker
        choices={choices}
        value="workstation"
        onChange={onChange}
      />,
    );
    const trigger = screen.getByRole("button", {
      name: "Environment: Workstation",
    });
    trigger.focus();
    fireEvent.keyDown(trigger, { key: "ArrowDown" });
    const workstation = screen.getByRole("menuitemradio", {
      name: /^Workstation/,
    });
    const minimal = screen.getByRole("menuitemradio", { name: /^Minimal/ });
    expect(workstation).toHaveFocus();
    fireEvent.keyDown(workstation, { key: "End" });
    expect(minimal).toHaveFocus();
    fireEvent.keyDown(minimal, { key: "Home" });
    expect(workstation).toHaveFocus();
    fireEvent.keyDown(workstation, { key: "m" });
    expect(minimal).toHaveFocus();
    fireEvent.keyDown(minimal, { key: "ArrowDown" });
    expect(workstation).toHaveFocus();
    fireEvent.keyDown(workstation, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("keeps a disabled picker closed and associates the tooltip reason", () => {
    const onChange = vi.fn();
    render(
      <>
        <EnvironmentPicker
          choices={choices}
          value="workstation"
          onChange={onChange}
          disabled
          disabledReason="Stop the current run before changing the environment."
        />
      </>,
    );
    const trigger = screen.getByRole("button", {
      name: "Environment: Workstation",
    });
    expect(trigger).toHaveAttribute("aria-disabled", "true");
    expect(trigger).toHaveAccessibleDescription(
      "Stop the current run before changing the environment.",
    );
    fireEvent.click(trigger);
    fireEvent.keyDown(trigger, { key: "ArrowDown" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("does not invent a selection when the catalogue is unavailable", () => {
    render(<EnvironmentPicker choices={[]} value="" onChange={vi.fn()} />);
    expect(
      screen.getByRole("button", { name: "Environment: Unavailable" }),
    ).toHaveAttribute("aria-disabled", "true");
    expect(screen.queryByRole("img", { name: /Capabilities/ })).toBeNull();
  });

  it("closes on an outside pointer action without selecting an option", () => {
    const onChange = vi.fn();
    render(
      <EnvironmentPicker
        choices={choices}
        value="minimal"
        onChange={onChange}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Environment: Minimal" }),
    );
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(onChange).not.toHaveBeenCalled();
  });

  // General-purpose finding N1: what each environment is for, in the administrator's words; its tools on hover.
  it("reads each environment by what it is for, with its tools on hover, or its tools when it says nothing", () => {
    render(
      <EnvironmentPicker
        choices={choices}
        value="workstation"
        onChange={vi.fn()}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Environment: Workstation" }),
    );
    const workstation = screen.getByRole("menuitemradio", {
      name: /^Workstation/,
    });
    // What it is for, then its size on the same line.
    const purpose = within(workstation).getByText(
      /^For most tasks: web pages, documents, data and code ·/,
    );
    expect(purpose).toHaveAttribute(
      "title",
      "Shell, Python, Node.js, Build tools, Browser",
    );
    expect(within(workstation).queryByText(/^Shell, Python/)).toBeNull();
    const minimal = screen.getByRole("menuitemradio", { name: /^Minimal/ });
    expect(within(minimal).getByText("Shell")).toBeVisible();
  });
});

describe("WEB-U-120 the data capability", () => {
  it("shows Data among Workstation's tools", () => {
    render(
      <EnvironmentPicker
        choices={choices.map((choice) =>
          choice.id === "workstation"
            ? { ...choice, capabilities: [...choice.capabilities, "data"] }
            : choice,
        )}
        value="workstation"
        onChange={vi.fn()}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Environment: Workstation" }),
    );
    const workstation = screen.getByRole("menuitemradio", {
      name: /^Workstation/,
    });
    expect(within(workstation).getByText(/^For most tasks: /)).toHaveAttribute(
      "title",
      "Shell, Python, Node.js, Build tools, Browser, Data",
    );
  });
});
