import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import { choose } from "../test/picker";

import { ChannelComposer } from "./ChannelComposer";
import type { ChannelPost } from "./contract";

function show(
  props: Partial<Parameters<typeof ChannelComposer>[0]> = {},
  post = vi.fn<(write: ChannelPost) => Promise<void>>(() => Promise.resolve()),
) {
  render(
    <ChannelComposer
      memberRole="member"
      state="open"
      replyTo={null}
      onCancelReply={vi.fn()}
      onPost={post}
      {...props}
    />,
  );
  return post;
}
const form = () => screen.getByRole("form", { name: "Post to the channel" });
/** A proposal or a task starts from its button under the message box (no kind-of-post radios). */
const kind = (label: "Proposal" | "Task") =>
  fireEvent.click(
    within(form()).getByRole("button", {
      name: label === "Proposal" ? "Propose…" : "Add a to-do…",
    }),
  );
/** Types into the field labelled `label`, or, for a standard picker (WEB-U-134), chooses the choice so worded. */
const type = (label: string, value: string) => {
  const box = within(form()).queryByLabelText(label);
  if (box) fireEvent.change(box, { target: { value } });
  else choose(label, value, form());
};
const send = () =>
  fireEvent.click(within(form()).getByRole("button", { name: "Post" }));

// ADR-0014: a person who may post writes a message, a proposal or a task, or replies; observers and ended channels
// read only (WEB-U-063).
describe("WEB-U-063 the channel composer", () => {
  it("posts a message, and nothing when it is empty", async () => {
    const post = show();
    send();
    expect(post).not.toHaveBeenCalled();
    type("Your message", "Starting on the notes.");
    send();
    await vi.waitFor(() =>
      expect(post).toHaveBeenCalledWith({
        body: { type: "text", text: "Starting on the notes." },
      }),
    );
    await vi.waitFor(() =>
      expect(within(form()).getByLabelText("Your message")).toHaveValue(""),
    );
  });

  it("is one message box, with Propose and Add a task turning it into a titled form and Cancel turning it back", () => {
    show();
    expect(within(form()).queryAllByRole("radio")).toEqual([]);
    expect(within(form()).queryByLabelText("Title")).toBeNull();
    kind("Proposal");
    expect(within(form()).getByLabelText("Title")).toHaveFocus();
    expect(within(form()).getByLabelText("What and why")).toBeVisible();
    expect(
      within(form()).queryByRole("button", { name: "Propose…" }),
    ).toBeNull();
    fireEvent.click(within(form()).getByRole("button", { name: "Cancel" }));
    expect(within(form()).queryByLabelText("Title")).toBeNull();
    expect(within(form()).getByLabelText("Your message")).toBeVisible();
    kind("Task");
    expect(within(form()).getByLabelText("What is to be done")).toBeVisible();
  });

  it("posts a proposal with its voting window", async () => {
    const post = show();
    kind("Proposal");
    type("Title", "Ship on Friday");
    type("What and why", "All green.");
    type("Votes close after", "30 minutes");
    send();
    await vi.waitFor(() =>
      expect(post).toHaveBeenCalledWith({
        body: {
          type: "proposal",
          title: "Ship on Friday",
          text: "All green.",
          window_seconds: 1800,
        },
      }),
    );
  });

  it("posts a task with how many may claim it", async () => {
    const post = show();
    kind("Task");
    type("Title", "Write the changelog");
    type("What is to be done", "From the merged PRs.");
    type("Who may take it on", "A number of members");
    type("How many", "3");
    send();
    await vi.waitFor(() =>
      expect(post).toHaveBeenLastCalledWith({
        body: {
          type: "task",
          title: "Write the changelog",
          spec: "From the merged PRs.",
          claims: 3,
        },
      }),
    );
    // After a post the form starts again from a message.
    await vi.waitFor(() =>
      expect(within(form()).getByLabelText("Your message")).toHaveValue(""),
    );
    kind("Task");
    type("Title", "Review it");
    type("What is to be done", "Each of you.");
    type("Who may take it on", "Every voting member");
    send();
    await vi.waitFor(() =>
      expect(post).toHaveBeenLastCalledWith({
        body: {
          type: "task",
          title: "Review it",
          spec: "Each of you.",
          claims: "all",
        },
      }),
    );
  });

  it("replies to a post, and can stop replying", async () => {
    const onCancelReply = vi.fn();
    const post = show({ replyTo: 7, onCancelReply });
    expect(within(form()).getByText("Replying to #7")).toBeVisible();
    type("Your message", "Looks right.");
    send();
    await vi.waitFor(() =>
      expect(post).toHaveBeenCalledWith({
        parent: 7,
        body: { type: "text", text: "Looks right." },
      }),
    );
    fireEvent.click(
      within(form()).getByRole("button", { name: "Stop replying" }),
    );
    expect(onCancelReply).toHaveBeenCalled();
  });

  it("says why when the post is refused, keeping the text", async () => {
    const post = show(
      {},
      vi.fn(() => Promise.reject(new ApiError(409, "channel_ended", "x"))),
    );
    type("Your message", "Late note");
    send();
    expect(
      await within(form()).findByText(
        "This channel has ended; nothing more can be posted.",
      ),
    ).toBeVisible();
    expect(within(form()).getByLabelText("Your message")).toHaveValue(
      "Late note",
    );
    expect(post).toHaveBeenCalledTimes(1);
  });

  it("reads only for an observer", () => {
    show({ memberRole: "observer" });
    expect(screen.queryByRole("form")).toBeNull();
    expect(
      screen.getByText("You can only read this channel, not post."),
    ).toBeVisible();
  });

  it("reads only on an ended channel", () => {
    show({ state: "ended" });
    expect(screen.queryByRole("form")).toBeNull();
    expect(
      screen.getByText("This channel has ended; it can be read, not added to."),
    ).toBeVisible();
  });

  it("reads only for an administrator who isn't a member", () => {
    show({ memberRole: null });
    expect(screen.queryByRole("form")).toBeNull();
    expect(
      screen.getByText("You can read this channel; only its members post."),
    ).toBeVisible();
  });
});
