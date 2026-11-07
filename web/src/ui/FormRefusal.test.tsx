import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState, type FormEvent } from "react";
import { describe, expect, it, vi } from "vitest";

import { FormRefusal } from "./FormRefusal";
import { Input } from "./Input";

const without = (all: Record<string, string>, key: string) =>
  Object.fromEntries(Object.entries(all).filter(([name]) => name !== key));

/** A form whose save refuses its empty fields at once, or once "the gateway" answers (`later`). */
function Form({ later = false }: { later?: boolean }) {
  const [name, setName] = useState("");
  const [model, setModel] = useState("");
  const [errors, setErrors] = useState<Record<string, string>>({});
  function submit(event: FormEvent) {
    event.preventDefault();
    const found = {
      ...(name ? {} : { name: "Give it a name." }),
      ...(model ? {} : { model: "Give it a model." }),
    };
    if (later) setTimeout(() => setErrors(found), 10);
    else setErrors(found);
  }
  return (
    <form noValidate onSubmit={submit}>
      <Input label="Other" defaultValue="fine" />
      <Input
        label="Name"
        value={name}
        onChange={(event) => {
          setName(event.target.value);
          setErrors((all) => without(all, "name"));
        }}
        {...(errors.name ? { error: errors.name } : {})}
      />
      <Input
        label="Model"
        value={model}
        onChange={(event) => {
          setModel(event.target.value);
          setErrors((all) => without(all, "model"));
        }}
        {...(errors.model ? { error: errors.model } : {})}
      />
      <FormRefusal />
      <button type="submit">Save</button>
    </form>
  );
}

// WEB-U-110 (the lead's first hour, F14): a save the form refuses never does "nothing": the first field to fix gets the
// focus and comes into view, and a line by Save says how many are left.
describe("WEB-U-110 a refused save shows where", () => {
  it("focuses the first invalid field, brings it into view, and counts what is left", () => {
    const scroll = vi.fn();
    Element.prototype.scrollIntoView = scroll;
    render(<Form />);
    expect(screen.getByRole("status")).toHaveTextContent("");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const name = screen.getByLabelText("Name");
    expect(name).toHaveFocus();
    expect(scroll).toHaveBeenCalledOnce();
    expect(scroll.mock.contexts[0]).toBe(name);
    expect(screen.getByRole("status")).toHaveTextContent("Fix 2 fields above.");
    fireEvent.change(name, { target: { value: "qwen" } });
    expect(screen.getByRole("status")).toHaveTextContent("Fix 1 field above.");
    // Fixing a field never moves the focus again.
    expect(name).toHaveFocus();
    fireEvent.change(screen.getByLabelText("Model"), {
      target: { value: "qwen3" },
    });
    expect(screen.getByRole("status")).toHaveTextContent("");
  });

  it("does the same when the refusal arrives later, from the gateway", async () => {
    vi.useFakeTimers();
    Element.prototype.scrollIntoView = vi.fn();
    render(<Form later />);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(screen.getByRole("button", { name: "Save" })).not.toHaveFocus();
    await act(() => vi.advanceTimersByTimeAsync(20));
    expect(screen.getByLabelText("Name")).toHaveFocus();
    expect(screen.getByRole("status")).toHaveTextContent("Fix 2 fields above.");
    vi.useRealTimers();
  });

  it("says nothing and leaves the focus before any save", () => {
    Element.prototype.scrollIntoView = vi.fn();
    render(<Form />);
    const other = screen.getByLabelText("Other");
    other.focus();
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "" } });
    expect(other).toHaveFocus();
    expect(screen.getByRole("status")).toHaveTextContent("");
  });
});
