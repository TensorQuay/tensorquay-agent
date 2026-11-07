import { fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { Input } from "./Input";
import { Select } from "./Select";
import { Textarea } from "./Textarea";

describe.each([
  ["Input", Input],
  ["Textarea", Textarea],
] as const)("%s", (_name, Field) => {
  it("associates its label with an editable field", () => {
    const onChange = vi.fn();
    render(<Field label="Name" onChange={onChange} />);
    fireEvent.change(screen.getByLabelText("Name"), {
      target: { value: "Example" },
    });
    expect(onChange).toHaveBeenCalledOnce();
  });
  it("exposes disabled state", () => {
    render(<Field label="Name" disabled />);
    expect(screen.getByLabelText("Name")).toBeDisabled();
  });
  it("announces and associates an error", () => {
    render(<Field label="Name" error="Enter a name." />);
    expect(screen.getByLabelText("Name")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
    expect(screen.getByLabelText("Name")).toHaveAccessibleDescription(
      "Enter a name.",
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Enter a name.");
  });
});

// WEB-U-134 (F19's rest): the shared Select is the standard picker, never a native <select>: a labelled field-looking
// button that opens the standard menu, keyboard operable, its error announced and marked for FormRefusal.
describe("WEB-U-134 Select is the standard picker", () => {
  function Chosen({ onChange }: { onChange?: (value: string) => void }) {
    const [value, setValue] = useState("default");
    return (
      <Select
        label="Workspace"
        value={value}
        onChange={(next) => {
          setValue(next);
          onChange?.(next);
        }}
      >
        <option value="default">Default</option>
        <option value="example">Example</option>
        <option value="gone" disabled>
          Gone
        </option>
      </Select>
    );
  }

  it("shows its label above a field-looking button that names the choice", () => {
    render(<Chosen />);
    expect(screen.getByText("Workspace")).toBeVisible();
    const picker = screen.getByRole("button", { name: "Workspace: Default" });
    expect(picker).toHaveClass("picker-field");
    expect(screen.queryByRole("combobox")).toBeNull();
    expect(document.querySelector("select")).toBeNull();
  });

  it("opens the standard menu and applies a choice by keyboard", () => {
    const onChange = vi.fn();
    render(<Chosen onChange={onChange} />);
    const picker = screen.getByRole("button", { name: "Workspace: Default" });
    fireEvent.keyDown(picker, { key: "ArrowDown" });
    const menu = screen.getByRole("menu", { name: /Workspace/ });
    expect(
      within(menu)
        .getAllByRole("menuitemradio")
        .map((item) => item.textContent),
    ).toEqual(["Default", "Example", "Gone Unavailable"]);
    fireEvent.click(within(menu).getByRole("menuitemradio", { name: /Gone/ }));
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.click(
      within(menu).getByRole("menuitemradio", { name: /Example/ }),
    );
    expect(onChange).toHaveBeenCalledWith("example");
    expect(
      screen.getByRole("button", { name: "Workspace: Example" }),
    ).toHaveFocus();
  });

  it("supports disabled and error states", () => {
    render(
      <Select
        label="Workspace"
        value="default"
        onChange={() => undefined}
        disabled
        error="Choose a workspace."
      >
        <option value="default">Default</option>
      </Select>,
    );
    const picker = screen.getByRole("button", { name: "Workspace: Default" });
    expect(picker).toHaveAttribute("aria-disabled", "true");
    expect(picker).toHaveAttribute("aria-invalid", "true");
    expect(picker).toHaveAccessibleDescription("Choose a workspace.");
    expect(screen.getByRole("alert")).toHaveTextContent("Choose a workspace.");
    fireEvent.click(picker);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("keeps an option's language on its row", () => {
    render(
      <Select label="Language" value="en" onChange={() => undefined}>
        <option value="en" lang="en">
          English
        </option>
        <option value="zh-Hans" lang="zh-Hans">
          简体中文
        </option>
      </Select>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Language: English" }));
    expect(screen.getByText("简体中文")).toHaveAttribute("lang", "zh-Hans");
  });

  it("keeps its own choice for a form that reads it by name", () => {
    const { container } = render(
      <form>
        <Select label="Expires in" name="days" defaultValue="30">
          <option value="7">7 days</option>
          <option value="30">30 days</option>
        </Select>
      </form>,
    );
    const form = container.querySelector("form");
    if (!form) throw new Error("The form must render");
    expect(new FormData(form).get("days")).toBe("30");
    fireEvent.click(
      screen.getByRole("button", { name: "Expires in: 30 days" }),
    );
    fireEvent.click(screen.getByRole("menuitemradio", { name: /7 days/ }));
    expect(new FormData(form).get("days")).toBe("7");
  });

  it("keeps a hint it is described by", () => {
    render(
      <>
        <Select
          label="Workspace"
          value="default"
          onChange={() => undefined}
          aria-describedby="hint"
        >
          <option value="default">Default</option>
        </Select>
        <p id="hint">Where the files go.</p>
      </>,
    );
    expect(
      screen.getByRole("button", { name: "Workspace: Default" }),
    ).toHaveAccessibleDescription("Where the files go.");
  });
});
