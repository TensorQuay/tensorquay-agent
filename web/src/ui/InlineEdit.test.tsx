import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { InlineEdit } from "./InlineEdit";

function show(
  commit: (value: string) => Promise<boolean> | boolean = () => true,
) {
  const onCommit = vi.fn(commit);
  const onCancel = vi.fn();
  render(
    <>
      <InlineEdit
        value="Clean the sales data"
        label="Task title"
        onCommit={onCommit}
        onCancel={onCancel}
      />
      <button>Elsewhere</button>
    </>,
  );
  const field = screen.getByRole("textbox", { name: "Task title" });
  return { field, onCommit, onCancel };
}

// The UI standard's In-place edit: the text itself becomes the field, all of it selected; Enter or a click elsewhere
// saves, Esc cancels; no visible label and no buttons.
describe("WEB-U-092 in-place edit", () => {
  it("is the text, focused and all selected, with no label or buttons to show", () => {
    const { field } = show();
    expect(field).toHaveValue("Clean the sales data");
    expect(field).toHaveFocus();
    expect((field as HTMLInputElement).selectionStart).toBe(0);
    expect((field as HTMLInputElement).selectionEnd).toBe(
      "Clean the sales data".length,
    );
    expect(document.querySelector("label")).toBeNull();
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });

  it("saves the trimmed text on Enter", async () => {
    const { field, onCommit, onCancel } = show();
    fireEvent.change(field, { target: { value: "  Sales cleanup " } });
    fireEvent.keyDown(field, { key: "Enter" });
    await vi.waitFor(() =>
      expect(onCommit).toHaveBeenCalledExactlyOnceWith("Sales cleanup"),
    );
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("saves once on a click elsewhere, though the field then also loses focus", async () => {
    const { field, onCommit } = show();
    fireEvent.change(field, { target: { value: "Sales cleanup" } });
    const elsewhere = screen.getByRole("button", { name: "Elsewhere" });
    fireEvent.pointerDown(elsewhere);
    fireEvent.blur(field);
    await vi.waitFor(() => expect(onCommit).toHaveBeenCalledOnce());
  });

  it("saves when focus moves away (Tab)", async () => {
    const { field, onCommit } = show();
    fireEvent.change(field, { target: { value: "Sales cleanup" } });
    fireEvent.blur(field);
    await vi.waitFor(() =>
      expect(onCommit).toHaveBeenCalledExactlyOnceWith("Sales cleanup"),
    );
  });

  it("cancels on Esc, and an empty or unchanged text changes nothing", () => {
    const first = show();
    fireEvent.change(first.field, { target: { value: "Something else" } });
    fireEvent.keyDown(first.field, { key: "Escape" });
    expect(first.onCancel).toHaveBeenCalledOnce();
    expect(first.onCommit).not.toHaveBeenCalled();
  });

  it.each(["   ", "Clean the sales data"])(
    "leaves the old text for %j",
    (value) => {
      const { field, onCommit, onCancel } = show();
      fireEvent.change(field, { target: { value } });
      fireEvent.keyDown(field, { key: "Enter" });
      expect(onCommit).not.toHaveBeenCalled();
      expect(onCancel).toHaveBeenCalledOnce();
    },
  );

  it("keeps the text to try again when saving fails", async () => {
    const { field, onCommit } = show(() => Promise.resolve(false));
    fireEvent.change(field, { target: { value: "Sales cleanup" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await vi.waitFor(() => expect(onCommit).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(field).not.toHaveAttribute("aria-busy"));
    expect(field).toHaveValue("Sales cleanup");
    fireEvent.keyDown(field, { key: "Enter" });
    await vi.waitFor(() => expect(onCommit).toHaveBeenCalledTimes(2));
  });
});

// WEB-U-114 (the title's in-place edit): the field shows a focus ring only when the keyboard opened it (the ⋯'s
// Rename chosen by keys); opened by a click it is the text with its thin underline and no ring.
describe("WEB-U-114 the in-place edit's focus ring", () => {
  it("has no ring when a pointer opened it", () => {
    fireEvent.pointerDown(document.body);
    const { field } = show();
    expect(field).not.toHaveAttribute("data-keyboard");
  });

  it("has its ring when the keyboard opened it", () => {
    fireEvent.keyDown(document.body, { key: "Enter" });
    const { field } = show();
    expect(field).toHaveAttribute("data-keyboard");
  });

  it("goes by the last input: a key after a click, a click after a key", () => {
    fireEvent.keyDown(document.body, { key: "Enter" });
    fireEvent.pointerDown(document.body);
    expect(show().field).not.toHaveAttribute("data-keyboard");
  });
});
