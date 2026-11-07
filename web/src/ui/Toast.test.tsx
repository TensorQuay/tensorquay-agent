import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Toast } from "./Toast";
import { createToastStore } from "./toast-store";

afterEach(() => vi.useRealTimers());

describe("Toast", () => {
  it("dismisses an informational message after five seconds", () => {
    vi.useFakeTimers();
    const store = createToastStore();
    render(<Toast store={store} />);
    act(() => {
      store.push({ message: "Changes saved", tone: "success" });
    });
    expect(
      screen.getByRole("region", { name: "Notifications" }),
    ).toHaveTextContent("Changes saved");
    act(() => {
      vi.advanceTimersByTime(4999);
    });
    expect(screen.getByText("Changes saved")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.queryByText("Changes saved")).not.toBeInTheDocument();
  });
  it("keeps errors until dismissed and supports an action", () => {
    vi.useFakeTimers();
    const store = createToastStore();
    const onClick = vi.fn();
    render(<Toast store={store} />);
    act(() => {
      store.push({
        message: "Could not save",
        tone: "error",
        action: { label: "Retry", onClick },
      });
    });
    act(() => {
      vi.advanceTimersByTime(10_000);
    });
    expect(screen.getByRole("alert")).toHaveTextContent("Could not save");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onClick).toHaveBeenCalledOnce();
    fireEvent.click(
      screen.getByRole("button", { name: "Dismiss Could not save" }),
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
  it("renders an informational message", () => {
    const store = createToastStore([
      { message: "Workspace ready", tone: "info" },
    ]);
    render(<Toast store={store} />);
    expect(screen.getByRole("status")).toHaveTextContent("Workspace ready");
  });
});
