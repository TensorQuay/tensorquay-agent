import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { Sparkles } from "lucide-react";
import { describe, expect, it, vi } from "vitest";

import { EnvironmentPicker } from "../conversations/EnvironmentPicker";
import { ModelPicker } from "../conversations/ModelPicker";

import { Dialog } from "./Dialog";
import { MenuButton } from "./MenuButton";

const options = [
  { id: "a", label: "Alpha" },
  { id: "b", label: "Beta" },
  { id: "c", label: "Bravo" },
];

describe("WEB-U-007 shared picker menu", () => {
  it("checks the current item and supports arrows, Enter, Escape and multi-letter typeahead", () => {
    const change = vi.fn();
    render(
      <MenuButton
        label="Model"
        options={options}
        value="a"
        onChange={change}
      />,
    );
    const trigger = screen.getByRole("button", { name: "Model: Alpha" });
    expect(screen.queryByRole("combobox")).toBeNull();
    fireEvent.keyDown(trigger, { key: "ArrowDown" });
    const alpha = screen.getByRole("menuitemradio", { name: "Alpha" });
    const beta = screen.getByRole("menuitemradio", { name: "Beta" });
    const bravo = screen.getByRole("menuitemradio", { name: "Bravo" });
    expect(alpha).toHaveAttribute("aria-checked", "true");
    expect(alpha).toHaveFocus();
    fireEvent.keyDown(alpha, { key: "ArrowDown" });
    expect(beta).toHaveFocus();
    fireEvent.keyDown(beta, { key: "End" });
    expect(bravo).toHaveFocus();
    fireEvent.keyDown(bravo, { key: "Home" });
    fireEvent.keyDown(alpha, { key: "b" });
    expect(beta).toHaveFocus();
    fireEvent.keyDown(beta, { key: "r" });
    expect(bravo).toHaveFocus();
    fireEvent.keyDown(bravo, { key: "Enter" });
    expect(change).toHaveBeenCalledExactlyOnceWith("c");
    expect(trigger).toHaveFocus();
    fireEvent.click(trigger);
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
    expect(change).toHaveBeenCalledOnce();
  });

  it("keeps a disabled trigger focusable to explain itself without opening", () => {
    render(
      <MenuButton
        label="Model"
        options={options}
        value="a"
        onChange={vi.fn()}
        disabledReason="Stop the current run before changing the model."
      />,
    );
    const trigger = screen.getByRole("button", { name: "Model: Alpha" });
    trigger.focus();
    expect(trigger).toHaveFocus();
    expect(trigger).toHaveAccessibleDescription(
      "Stop the current run before changing the model.",
    );
    fireEvent.click(trigger);
    fireEvent.keyDown(trigger, { key: "ArrowDown" });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("closes when keyboard focus leaves without pulling focus back from the next control", () => {
    render(
      <>
        <MenuButton
          label="Model"
          options={options}
          value="a"
          onChange={vi.fn()}
        />
        <button type="button">Next control</button>
      </>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Model: Alpha" }));
    const next = screen.getByRole("button", { name: "Next control" });
    act(() => next.focus());
    expect(screen.queryByRole("menu")).toBeNull();
    expect(next).toHaveFocus();
  });

  it("prefers the display name and falls back to the model name, never the route identity", () => {
    const model = {
      name: "route-identity",
      model: "Model name",
      protocol: "openai_chat" as const,
      display_name: null,
      verified: null,
      context_window: null,
      description: null,
      reasoning: null,
      vision: false,
      native_tools: true,
      runs_on: { where: "hosted" as const, provider: null },
    };
    const { rerender } = render(
      <ModelPicker
        models={[{ ...model, display_name: "Display name" }]}
        route={model.name}
        onChange={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("button", { name: "Model: Display name" }),
    ).toBeVisible();
    rerender(
      <ModelPicker models={[model]} route={model.name} onChange={vi.fn()} />,
    );
    expect(
      screen.getByRole("button", { name: "Model: Model name" }),
    ).toBeVisible();
    expect(screen.queryByText(model.name)).toBeNull();
  });

  // General-purpose finding X1: a model's purpose line, in the administrator's words, under its name in the menu.
  it("shows a model's description under its name in the menu", () => {
    const model = {
      name: "fast",
      display_name: "Fast model",
      description: "Quick answers for short everyday tasks",
      model: "fast-model",
      protocol: "openai_chat" as const,
      verified: null,
      context_window: null,
      reasoning: null,
      vision: false,
      native_tools: true,
      runs_on: { where: "hosted" as const, provider: null },
    };
    render(<ModelPicker models={[model]} route="fast" onChange={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Model: Fast model" }));
    expect(
      within(
        screen.getByRole("menuitemradio", { name: /^Fast model/ }),
      ).getByText("Quick answers for short everyday tasks"),
    ).toBeVisible();
  });
});

// WEB-U-085 (the UI standard's Popover): a picker past 7 choices has a search field, a choice that can't be taken says
// why, and it is dismissed the one shared way (useDismiss): Esc or a press outside, never a Done.
const many = [
  "Alpha",
  "Beta",
  "Gamma",
  "Delta",
  "Epsilon",
  "Zeta",
  "Eta",
  "Theta",
].map((label) => ({
  id: label.toLowerCase(),
  label,
  detail: <span>{label === "Gamma" ? "the local one" : "hosted"}</span>,
}));

describe("WEB-U-085 the shared picker", () => {
  // A picker for a new thing's value (an environment's image, WEB-U-097) starts with nothing chosen and still opens.
  it("says what to do while nothing is chosen, opens, and checks nothing", () => {
    const change = vi.fn();
    render(
      <MenuButton
        label="Image"
        options={options}
        value=""
        placeholder="Choose an image"
        onChange={change}
      />,
    );
    const trigger = screen.getByRole("button", {
      name: "Image: Choose an image",
    });
    expect(trigger).not.toHaveAttribute("aria-disabled");
    fireEvent.click(trigger);
    const choices = screen.getAllByRole("menuitemradio");
    expect(choices).toHaveLength(3);
    for (const choice of choices)
      expect(choice).toHaveAttribute("aria-checked", "false");
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Beta" }));
    expect(change).toHaveBeenCalledExactlyOnceWith("b");
  });

  it("stays closed without a placeholder when its value is none of the choices", () => {
    render(
      <MenuButton
        label="Image"
        options={options}
        value=""
        onChange={vi.fn()}
      />,
    );
    const trigger = screen.getByRole("button", { name: /^Image: / });
    expect(trigger).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(trigger);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("has no search field for 7 choices or fewer", () => {
    render(
      <MenuButton
        label="Model"
        options={many.slice(0, 7)}
        value="alpha"
        onChange={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Model: Alpha" }));
    expect(screen.queryByRole("searchbox")).toBeNull();
  });

  it("finds a choice past 7 by its name or its line, and says so when nothing matches", () => {
    const change = vi.fn();
    render(
      <MenuButton
        label="Model"
        options={many}
        value="alpha"
        onChange={change}
      />,
    );
    const trigger = screen.getByRole("button", { name: "Model: Alpha" });
    fireEvent.click(trigger);
    const search = screen.getByRole("searchbox", { name: "Find" });
    expect(search).toHaveFocus();
    fireEvent.change(search, { target: { value: "local" } });
    expect(
      screen.getAllByRole("menuitemradio").map((row) => row.textContent),
    ).toEqual([expect.stringContaining("Gamma")]);
    fireEvent.change(search, { target: { value: "et" } });
    expect(
      screen
        .getAllByRole("menuitemradio")
        .map((row) => row.querySelector("span > span")?.textContent),
    ).toEqual(["Beta", "Zeta", "Eta", "Theta"]);
    // Down from the field goes to the first match; Enter in the field takes the first match.
    fireEvent.keyDown(search, { key: "ArrowDown" });
    expect(screen.getAllByRole("menuitemradio")[0]).toHaveFocus();
    search.focus();
    fireEvent.keyDown(search, { key: "Enter" });
    expect(change).toHaveBeenCalledExactlyOnceWith("beta");
    expect(trigger).toHaveFocus();
    fireEvent.click(trigger);
    fireEvent.change(screen.getByRole("searchbox", { name: "Find" }), {
      target: { value: "omega" },
    });
    expect(screen.queryAllByRole("menuitemradio")).toEqual([]);
    expect(screen.getByText("No match")).toBeVisible();
  });

  it("shows why a choice can't be taken, and doesn't take it", () => {
    const change = vi.fn();
    render(
      <MenuButton
        label="Model"
        options={[
          { id: "a", label: "Alpha" },
          { id: "b", label: "Beta", disabled: "Its key is missing" },
        ]}
        value="a"
        onChange={change}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Model: Alpha" }));
    const beta = screen.getByRole("menuitemradio", { name: /Beta/ });
    expect(beta).toHaveAttribute("aria-disabled", "true");
    expect(beta).toHaveTextContent("Its key is missing");
    fireEvent.click(beta);
    expect(change).not.toHaveBeenCalled();
    expect(screen.getByRole("menu")).toBeVisible();
  });

  it("closes on a press outside, and on Esc without closing a dialog under it", () => {
    const closeDialog = vi.fn();
    render(
      <>
        <button type="button">Elsewhere</button>
        <Dialog open title="Choose" onClose={closeDialog}>
          <MenuButton
            label="Model"
            options={options}
            value="a"
            onChange={vi.fn()}
          />
        </Dialog>
      </>,
    );
    const trigger = screen.getByRole("button", { name: "Model: Alpha" });
    fireEvent.click(trigger);
    fireEvent.pointerDown(screen.getByRole("heading", { name: "Choose" }));
    expect(screen.queryByRole("menu")).toBeNull();
    fireEvent.click(trigger);
    fireEvent.keyDown(screen.getByRole("menuitemradio", { name: "Alpha" }), {
      key: "Escape",
    });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
    expect(closeDialog).not.toHaveBeenCalled();
  });
});

// A narrow task header's pickers (the stylesheet shows the icon there and hides the name): the chosen name is never cut,
// it is in the button's name, its tooltip and, while the icon shows, its menu's title.
describe("WEB-U-106 a picker that can show as its icon", () => {
  it("carries its icon, and names the choice in its tooltip and its menu's title", () => {
    const { container } = render(
      <MenuButton
        label="Model"
        icon={Sparkles}
        options={options}
        value="a"
        onChange={vi.fn()}
      />,
    );
    const trigger = screen.getByRole("button", { name: "Model: Alpha" });
    expect(trigger.querySelector(".picker-icon")).not.toBeNull();
    // Where the name has room the menu needs no title.
    fireEvent.click(trigger);
    expect(document.querySelector(".picker-title")).toBeNull();
    fireEvent.click(trigger);
    // The stylesheet shows the icon for the name where the header is narrow.
    trigger
      .querySelector<HTMLElement>(".picker-name")
      ?.style.setProperty("display", "none");
    // The tooltip repeats the button's name, so assistive technology reads it once.
    const tip = container.querySelector(".picker-name-tip");
    expect(tip).toHaveTextContent("Model: Alpha");
    expect(tip?.closest("[aria-hidden='true']")).not.toBeNull();
    fireEvent.click(trigger);
    expect(
      within(screen.getByRole("region", { name: "Model choices" })).getByText(
        "Model: Alpha",
      ),
    ).toHaveClass("picker-title");
  });

  it("keeps a disabled picker's reason as its description, beside the name", () => {
    const { container } = render(
      <MenuButton
        label="Model"
        icon={Sparkles}
        options={options}
        value="a"
        onChange={vi.fn()}
        disabledReason="Set by the agent profile"
      />,
    );
    expect(
      screen.getByRole("button", { name: "Model: Alpha" }),
    ).toHaveAccessibleDescription("Set by the agent profile");
    expect(container.querySelector(".picker-name-tip")).toHaveTextContent(
      "Model: Alpha",
    );
  });

  it("has no icon, tooltip name or title without one", () => {
    const { container } = render(
      <MenuButton
        label="Model"
        options={options}
        value="a"
        onChange={vi.fn()}
      />,
    );
    expect(
      container.querySelector(".picker-icon, .picker-name-tip"),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Model: Alpha" }));
    expect(document.querySelector(".picker-title")).toBeNull();
  });

  it("is given one for the model and the environment", () => {
    render(
      <>
        <ModelPicker
          route="local"
          onChange={vi.fn()}
          models={[
            {
              name: "local",
              display_name: "Local model",
              description: null,
              verified: null,
              model: "qwen3",
              protocol: "openai_chat",
              context_window: null,
              vision: false,
              native_tools: true,
              reasoning: null,
              runs_on: { where: "this_computer", provider: null },
            },
          ]}
        />
        <EnvironmentPicker
          choices={[
            {
              id: "w",
              label: "Workstation",
              capabilities: [],
              description: null,
            },
          ]}
          value="w"
          onChange={vi.fn()}
        />
      </>,
    );
    for (const name of ["Model: Local model", "Environment: Workstation"])
      expect(
        screen.getByRole("button", { name }).querySelector(".picker-icon"),
      ).not.toBeNull();
  });
});

// WEB-U-134: a list with sections (the Settings sections on a phone) is the same picker, its choices in named groups.
describe("WEB-U-134 grouped choices", () => {
  it("shows each group under its name and moves through them as one list", () => {
    const change = vi.fn();
    render(
      <MenuButton
        label="Section"
        value="account"
        onChange={change}
        options={[
          { id: "account", label: "Account", group: "Your account" },
          { id: "keys", label: "Keys", group: "Your account" },
          { id: "people", label: "People", group: "Administration" },
        ]}
      />,
    );
    fireEvent.keyDown(
      screen.getByRole("button", { name: "Section: Account" }),
      { key: "ArrowDown" },
    );
    const yours = screen.getByRole("group", { name: "Your account" });
    expect(
      within(yours)
        .getAllByRole("menuitemradio")
        .map((item) => item.textContent),
    ).toEqual(["Account", "Keys"]);
    expect(screen.getByText("Administration")).toBeVisible();
    const account = screen.getByRole("menuitemradio", { name: "Account" });
    expect(account).toHaveFocus();
    fireEvent.keyDown(account, { key: "ArrowDown" });
    fireEvent.keyDown(screen.getByRole("menuitemradio", { name: "Keys" }), {
      key: "ArrowDown",
    });
    const people = within(
      screen.getByRole("group", { name: "Administration" }),
    ).getByRole("menuitemradio", { name: "People" });
    expect(people).toHaveFocus();
    fireEvent.keyDown(people, { key: "Enter" });
    expect(change).toHaveBeenCalledWith("people");
  });
});
