import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Box } from "lucide-react";
import { describe, expect, it, vi } from "vitest";

import { useAttachments } from "../files/use-attachments";
import en from "../i18n/locales/en.json";
import type { Upload } from "../protocol/generated/api";
import { conversationId } from "../test/frames";
import type { MenuAction } from "../ui/ActionMenu";

import { Composer } from "./Composer";
import type { Transcript } from "./types";

const idle: Transcript["run"] = { id: null, status: "idle" };
const upload = (_id: string, file: File) =>
  Promise.resolve<Upload>({
    path: `uploads/${file.name}`,
    name: file.name,
    size: file.size,
  });

function Harness({
  more,
  run = idle,
  sending = false,
}: {
  more?: readonly MenuAction[];
  run?: Transcript["run"];
  sending?: boolean;
}) {
  const attachments = useAttachments(conversationId, upload);
  return (
    <Composer
      run={run}
      sandbox="ready"
      sending={sending}
      cancelling={false}
      onStop={() => Promise.resolve()}
      onSend={() => Promise.resolve(true)}
      attachments={attachments}
      more={more}
    />
  );
}
const button = (name: string) => screen.getByRole("button", { name });
const tooltip = () => document.querySelector(".action-tooltip");
/** The pointer over the button's holder (IconAction shows its tip from there). */
const hover = (element: HTMLElement) =>
  fireEvent.pointerEnter(element.parentElement as HTMLElement);
const leave = (element: HTMLElement) =>
  fireEvent.pointerLeave(element.parentElement as HTMLElement);
const iconSize = (element: HTMLElement) =>
  element.querySelector("svg")?.getAttribute("width");
const type = (text: string) =>
  fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
    target: { value: text },
  });

// WEB-U-428: the + and Send are one icon button (the UI standard's composer): 36 px round, a 20 px icon, a tooltip,
// and Send filled with ink only while it can send.
describe("WEB-U-428 the composer bar's icon buttons", () => {
  it("gives + and Send the same round icon button, each with a 20 px icon and a tooltip", () => {
    render(<Harness />);
    const plus = button("Attach files");
    const send = button("Send");
    for (const each of [plus, send]) {
      expect(each).toHaveClass("icon-button");
      expect(each).toHaveAttribute("data-shape", "round");
      expect(iconSize(each)).toBe("20");
      expect(each.querySelector("svg")).toHaveAttribute("stroke-width", "2");
    }
    expect(plus).toHaveAccessibleDescription(
      "Up to 25 MB per file, 32 files per message",
    );
    hover(plus);
    expect(tooltip()).toHaveTextContent("Attach files");
    expect(tooltip()).toHaveAttribute("aria-hidden", "true");
    // A layer of its own (WEB-U-429), never content of the bar or the card.
    expect(
      screen
        .getByRole("form", { name: "Message composer" })
        .contains(tooltip()),
    ).toBe(false);
    leave(plus);
    expect(tooltip()).toBeNull();
    // Empty, Send says what it waits for; with text, how to send.
    hover(send);
    expect(tooltip()).toHaveTextContent(en.messages["composer.sendEmptyTip"]);
    leave(send);
    type("Inspect the workspace");
    hover(send);
    expect(tooltip()).toHaveTextContent(en.messages["composer.sendTip"]);
    expect(en.messages["composer.sendTip"]).toBe("Send · Enter");
  });

  it("fills Send with ink only while there is text, and keeps the disc otherwise", async () => {
    const { rerender } = render(<Harness />);
    const send = button("Send");
    expect(send).toBeDisabled();
    expect(send).toHaveAttribute("data-variant", "primary");
    expect(send).toHaveAttribute("data-fill", "outline");
    type("Inspect the workspace");
    expect(send).toBeEnabled();
    expect(send).toHaveAttribute("data-fill", "ink");
    type("");
    expect(send).toHaveAttribute("data-fill", "outline");
    // An attachment alone leaves nothing to send: the disc stays an outline (the message needs its words).
    fireEvent.change(screen.getByLabelText("Choose files to attach"), {
      target: { files: [new File(["a"], "notes.txt", { type: "text/plain" })] },
    });
    await waitFor(() => expect(screen.getByText("notes.txt")).toBeVisible());
    expect(send).toBeDisabled();
    expect(send).toHaveAttribute("data-fill", "outline");
    // While a run is active the same button is Stop: a square on a bordered disc, as before.
    rerender(<Harness run={{ id: "run-example", status: "running" }} />);
    const stop = button("Stop");
    expect(stop).toHaveAttribute("data-shape", "round");
    expect(stop).toHaveAttribute("data-variant", "secondary");
    expect(iconSize(stop)).toBe("20");
    hover(stop);
    expect(tooltip()).toHaveTextContent("Stop");
  });

  it("keeps the blocked Send focusable with its outline, and disables it once the focus leaves", () => {
    render(<Harness />);
    const send = button("Send");
    type("Go");
    send.focus();
    expect(send).toHaveFocus();
    type("");
    expect(send).not.toBeDisabled();
    expect(send).toHaveAttribute("aria-disabled", "true");
    expect(send).toHaveAttribute("data-fill", "outline");
    fireEvent.blur(send);
    expect(send).toBeDisabled();
  });

  it("gives New task's + the same button, with a tip that says what it offers", () => {
    render(
      <Harness
        more={[
          {
            id: "workspace",
            label: "Workspace…",
            icon: Box,
            onSelect: vi.fn(),
          },
        ]}
      />,
    );
    const add = button("Add");
    expect(add).toHaveClass("icon-button");
    expect(add).toHaveAttribute("data-shape", "round");
    expect(iconSize(add)).toBe("20");
    hover(add);
    expect(tooltip()).toHaveTextContent(en.messages["attach.addTip"]);
    expect(en.messages["attach.addTip"]).toMatch(/^Attach files/);
  });
});
