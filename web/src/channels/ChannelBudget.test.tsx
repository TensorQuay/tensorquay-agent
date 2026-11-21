import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import { channel } from "../test/channels";

import { ChannelBudget } from "./ChannelBudget";

const PRICING = channel("pricing", {
  wake_budget: 300_000,
  spent_tokens: 114_000,
});

// WEB-U-529 (decided on 20 Nov 2026, C-1): a channel's wake budget on the page. Its owner, a person who leads it, or
// an administrator sets it from "Wake budget…" (what it is and what is spent, the number); everyone else reads who can.
describe("WEB-U-529 a channel's wake budget", () => {
  it("lets one who may set it: shows the budget and the spend, and sends the number", async () => {
    const save = vi.fn<(budget: number) => Promise<void>>(() =>
      Promise.resolve(),
    );
    const close = vi.fn();
    render(
      <ChannelBudget channel={PRICING} may onSave={save} onClose={close} />,
    );
    const dialog = screen.getByRole("dialog", { name: "Wake budget" });
    expect(dialog).toHaveTextContent(
      "114,000 of 300,000 tokens used today (38 %)",
    );
    const box = screen.getByRole("spinbutton", {
      name: "Tokens woken agents may use here each day",
    });
    expect(box).toHaveValue(300000);
    fireEvent.change(box, { target: { value: "600000" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await vi.waitFor(() => expect(save).toHaveBeenCalledWith(600000));
    await vi.waitFor(() => expect(close).toHaveBeenCalled());
  });

  it("says a refusal in words, and keeps the dialog open", async () => {
    const save = vi.fn(() =>
      Promise.reject(
        new ApiError(
          400,
          "invalid_request",
          "A channel's budget is between 0 (no agent is woken) and 10,000,000 tokens.",
        ),
      ),
    );
    render(
      <ChannelBudget channel={PRICING} may onSave={save} onClose={vi.fn()} />,
    );
    fireEvent.change(screen.getByRole("spinbutton"), {
      target: { value: "20000000" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    // In the reader's words, not the gateway's English.
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Enter a whole number of tokens, from 0 (no agent is woken) up to this installation's limit.",
    );
  });

  it("tells everyone else who can change it, with no field", () => {
    render(
      <ChannelBudget
        channel={PRICING}
        may={false}
        onSave={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    const dialog = screen.getByRole("dialog", { name: "Wake budget" });
    expect(dialog).toHaveTextContent(
      "114,000 of 300,000 tokens used today (38 %)",
    );
    expect(dialog).toHaveTextContent(
      "The channel's owner, a lead or an administrator can change the budget.",
    );
    expect(screen.queryByRole("spinbutton")).toBeNull();
  });
});
