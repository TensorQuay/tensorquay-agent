import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  channel,
  fakeChannels,
  FakeStream,
  member,
  message,
} from "../test/channels";

import { ChannelPage } from "./ChannelPage";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => "/channels/x",
}));

const CLOSE = channel("October close", { role: "member", member_count: 2 });
const KEEPER = member("conversation", "Bookkeeper", { owner_name: "ana" });
const ME = member("user", "Ana", { role: "member" });
const VIEW = 500;

/**
 * The page inside a scrolling area of VIEW px whose content is `height()` px tall (jsdom lays nothing out), as the app's
 * main area holds a channel. Returns the area.
 */
function show(
  api: Parameters<typeof ChannelPage>[0]["api"],
  height: () => number,
) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <div
        data-testid="area"
        style={{ overflowY: "auto" }}
        ref={(element) => {
          if (!element) return;
          Object.defineProperty(element, "scrollHeight", {
            configurable: true,
            get: height,
          });
          Object.defineProperty(element, "clientHeight", {
            configurable: true,
            get: () => VIEW,
          });
        }}
      >
        <ChannelPage id={CLOSE.id} api={api} />
      </div>
    </QueryClientProvider>,
  );
  return screen.getByTestId("area");
}

const posts = (count: number) =>
  Array.from({ length: count }, (_, index) =>
    message(index + 1, KEEPER, { type: "text", text: `Item ${index + 1}` }),
  );

// TEST: WEB-U-541
// 18.2.1: a channel opens at its newest message and follows new ones while the reader is at the bottom, as a task's
// conversation does. On 18.2 it opened at its oldest (the lead's live check, 23 Nov 2026).
describe("WEB-U-541 a channel opens at its newest message", () => {
  it("opens at the bottom once its log is read", async () => {
    const fake = fakeChannels({
      channels: [CLOSE],
      members: { [CLOSE.id]: [KEEPER, ME] },
      messages: { [CLOSE.id]: posts(31) },
    });
    const area = show(fake.api, () => 6000);
    await screen.findByText("Item 31");
    await vi.waitFor(() => expect(area.scrollTop).toBe(6000 - VIEW));
  });

  it("follows a new message at the bottom, and leaves a reader who scrolled up where they are", async () => {
    let height = 6000;
    const fake = fakeChannels({
      channels: [CLOSE],
      members: { [CLOSE.id]: [KEEPER, ME] },
      messages: { [CLOSE.id]: posts(31) },
    });
    const area = show(fake.api, () => height);
    await screen.findByText("Item 31");
    await vi.waitFor(() => expect(fake.streams).toHaveLength(1));
    const stream = fake.streams[0] as FakeStream;
    await vi.waitFor(() => expect(area.scrollTop).toBe(height - VIEW));
    height = 6200;
    act(() =>
      stream.send({
        type: "channel.message",
        ...message(32, KEEPER, { type: "text", text: "Item 32" }),
      }),
    );
    await screen.findByText("Item 32");
    expect(area.scrollTop).toBe(6200 - VIEW);
    // The reader scrolls up to read: a new message doesn't move them.
    area.scrollTop = 1200;
    fireEvent.scroll(area);
    height = 6400;
    act(() =>
      stream.send({
        type: "channel.message",
        ...message(33, KEEPER, { type: "text", text: "Item 33" }),
      }),
    );
    await screen.findByText("Item 33");
    expect(area.scrollTop).toBe(1200);
    // Back at the bottom, it follows again.
    area.scrollTop = 6400 - VIEW;
    fireEvent.scroll(area);
    height = 6600;
    act(() =>
      stream.send({
        type: "channel.message",
        ...message(34, KEEPER, { type: "text", text: "Item 34" }),
      }),
    );
    await screen.findByText("Item 34");
    expect(area.scrollTop).toBe(6600 - VIEW);
  });

  it("brings the person's own post into view, even from higher up", async () => {
    let height = 6000;
    const fake = fakeChannels({
      channels: [CLOSE],
      members: { [CLOSE.id]: [KEEPER, ME] },
      messages: { [CLOSE.id]: posts(31) },
    });
    const area = show(fake.api, () => height);
    await screen.findByText("Item 31");
    await vi.waitFor(() => expect(area.scrollTop).toBe(6000 - VIEW));
    area.scrollTop = 800;
    fireEvent.scroll(area);
    height = 6300;
    fireEvent.change(screen.getByLabelText("Your message"), {
      target: { value: "Thanks, posted the bank fee." },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByText("Thanks, posted the bank fee.");
    await vi.waitFor(() => expect(area.scrollTop).toBe(6300 - VIEW));
  });
});
