import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import { choose } from "../test/picker";

import { ChannelComposer } from "./ChannelComposer";
import type { ChannelPost, ChannelUpload } from "./contract";

type Post = (write: ChannelPost, files: readonly string[]) => Promise<void>;

function show(
  props: Partial<Parameters<typeof ChannelComposer>[0]> = {},
  post = vi.fn<Post>(() => Promise.resolve()),
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
/** Chooses Message, Propose or To-do (the composer's modes, WEB-U-524). */
const mode = (label: "Message" | "Propose" | "To-do") =>
  fireEvent.click(within(form()).getByRole("button", { name: label }));
/** Types into the field labelled `label`, or, for a standard picker (WEB-U-134), chooses the choice so worded. */
const type = (label: string, value: string) => {
  const box = within(form()).queryByLabelText(label);
  if (box) fireEvent.change(box, { target: { value } });
  else choose(label, value, form());
};
const send = () =>
  fireEvent.click(within(form()).getByRole("button", { name: "Send" }));

// ADR-0014; WEB-U-063, as 18.2's composer has it (WEB-U-524, the mock of 17 Nov 2026): one box, Message, Propose or
// To-do, the first line of a proposal or to-do its title.
describe("WEB-U-063 the channel composer", () => {
  it("posts a message, and nothing when it is empty", async () => {
    const post = show();
    send();
    expect(post).not.toHaveBeenCalled();
    type("Your message", "Hello.");
    send();
    await vi.waitFor(() =>
      expect(post).toHaveBeenCalledWith(
        { body: { type: "text", text: "Hello." } },
        [],
      ),
    );
  });

  it("is one box whose mode makes a proposal or a to-do of it, its first line the title", () => {
    show();
    expect(
      within(form()).getByRole("button", { name: "Message" }),
    ).toHaveAttribute("aria-pressed", "true");
    mode("Propose");
    expect(
      within(form()).getByLabelText("Proposal: its first line is the title"),
    ).toBeVisible();
    mode("To-do");
    expect(
      within(form()).getByLabelText("To-do: its first line is the title"),
    ).toBeVisible();
    mode("Message");
    expect(within(form()).getByLabelText("Your message")).toBeVisible();
  });

  it("posts a proposal with its voting window", async () => {
    const post = show();
    mode("Propose");
    type(
      "Proposal: its first line is the title",
      "Ship Friday\nThe build is green.",
    );
    type("Votes close after", "30 minutes");
    send();
    await vi.waitFor(() =>
      expect(post).toHaveBeenCalledWith(
        {
          body: {
            type: "proposal",
            title: "Ship Friday",
            text: "The build is green.",
            window_seconds: 1800,
          },
        },
        [],
      ),
    );
  });

  it("posts a to-do with how many may take it on, its title as its spec when it has one line", async () => {
    const post = show();
    mode("To-do");
    type("To-do: its first line is the title", "Write the changelog");
    type("Who may take it on", "A number of members");
    type("How many", "3");
    send();
    await vi.waitFor(() =>
      expect(post).toHaveBeenCalledWith(
        {
          body: {
            type: "task",
            title: "Write the changelog",
            spec: "Write the changelog",
            claims: 3,
          },
        },
        [],
      ),
    );
  });

  it("keeps a too long first line, saying the title's bound", async () => {
    const post = show();
    mode("Propose");
    type("Proposal: its first line is the title", "x".repeat(201));
    send();
    expect(
      await within(form()).findByText(
        "The first line is the title: keep it to 200 characters.",
      ),
    ).toBeVisible();
    expect(post).not.toHaveBeenCalled();
  });

  it("replies to a post, and can stop replying", async () => {
    const onCancelReply = vi.fn();
    const post = show({ replyTo: 7, onCancelReply });
    expect(within(form()).getByText("Replying to #7")).toBeVisible();
    type("Your message", "Looks right.");
    send();
    await vi.waitFor(() =>
      expect(post).toHaveBeenCalledWith(
        { parent: 7, body: { type: "text", text: "Looks right." } },
        [],
      ),
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

const file = (name: string, size = 2048) =>
  new File([new Uint8Array(size)], name);
const choose_files = (...files: File[]) =>
  fireEvent.change(
    within(form()).getByLabelText("Choose files to attach", {
      selector: "input",
    }),
    {
      target: { files },
    },
  );

// WEB-U-524 (18.2 slice 3; ADR-0014's amendment of 20 Nov 2026, A9): files are attached as they are chosen, each a chip
// with its size that can be removed (the upload goes too), and sent with the post; a refusal says why in plain words.
describe("WEB-U-524 files on a post", () => {
  it("attaches, removes and sends files with the post", async () => {
    let n = 0;
    const upload = vi.fn((chosen: File) =>
      Promise.resolve<ChannelUpload>({
        id: `u-${++n}`,
        name: chosen.name,
        size: chosen.size,
      }),
    );
    const remove = vi.fn(() => Promise.resolve());
    const post = show({ onUpload: upload, onRemoveUpload: remove });
    choose_files(file("q3.pdf"), file("notes.txt", 3));
    const chips = await within(form()).findByRole("list", {
      name: "Files to send",
    });
    await vi.waitFor(() =>
      expect(within(chips).getAllByRole("listitem")).toHaveLength(2),
    );
    expect(chips).toHaveTextContent("q3.pdf · 2 kB");
    fireEvent.click(
      within(chips).getByRole("button", { name: "Remove notes.txt" }),
    );
    expect(remove).toHaveBeenCalledWith("u-2");
    type("Your message", "The price list.");
    send();
    await vi.waitFor(() =>
      expect(post).toHaveBeenCalledWith(
        { body: { type: "text", text: "The price list." } },
        ["u-1"],
      ),
    );
    await vi.waitFor(() =>
      expect(
        within(form()).queryByRole("list", { name: "Files to send" }),
      ).toBeNull(),
    );
  });

  it("says why a file wasn't attached", async () => {
    const upload = vi.fn((chosen: File) =>
      Promise.reject(
        chosen.name === "huge.zip"
          ? new ApiError(413, "upload_too_large", "x")
          : new ApiError(409, "uploads_full", "x"),
      ),
    );
    show({ onUpload: upload });
    choose_files(file("huge.zip"));
    expect(
      await within(form()).findByText(
        "huge.zip is larger than a file may be here.",
      ),
    ).toBeVisible();
    choose_files(file("more.pdf"));
    expect(
      await within(form()).findByText(
        "This channel has no room for more.pdf: its files are full. Remove a file you haven't sent, or ask its owner to make room.",
      ),
    ).toBeVisible();
  });
});
