import { fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import type { Workspace } from "../protocol/generated/api";
import { sharedWorkspace, workspace } from "../test/workspaces";

import { WorkspacePicker } from "./WorkspacePicker";

function Harness({
  items,
  onChange,
}: {
  items: Workspace[];
  onChange: (id: string) => void;
}) {
  const [value, setValue] = useState("");
  return (
    <WorkspacePicker
      workspaces={items}
      value={value}
      onChange={(id) => {
        setValue(id);
        onChange(id);
      }}
    />
  );
}
function show(items: Workspace[]) {
  const onChange = vi.fn();
  render(<Harness items={items} onChange={onChange} />);
  return { onChange };
}
const open = (name = "Workspace: New workspace") =>
  fireEvent.click(screen.getByRole("button", { name }));
const lease = {
  conversation_id: "0195d8c0-0000-7000-8000-0000000000c9",
  title: null,
  running: true,
  pinned: false,
};

// Task 076: the workspace choice in the one-line task bar.
describe("WEB-U-025 workspace picker", () => {
  it("defaults to a new workspace and lists recent ones with owner and state", () => {
    const sales = workspace({
      name: "Sales analysis",
      last_used_at: "2026-01-05T00:00:00Z",
    });
    const handbook = sharedWorkspace("write", {
      name: "Team handbook",
      lease,
    });
    show([handbook, sales]);
    open();
    const menu = screen.getByRole("menu", { name: "Workspace" });
    const items = within(menu).getAllByRole("menuitemradio");
    expect(items.map((item) => item.textContent)).toEqual([
      "New workspace",
      "Sales analysisYou",
      "Team handbookana@example.invalid · In use",
    ]);
    expect(items[0]).toHaveAttribute("aria-checked", "true");
  });

  it("shows the busy state on the button once a workspace in use is chosen", () => {
    const busy = workspace({ name: "Data pipeline", lease });
    const { onChange } = show([busy]);
    open();
    fireEvent.click(
      screen.getByRole("menuitemradio", { name: /Data pipeline/ }),
    );
    expect(onChange).toHaveBeenCalledWith(busy.id);
    const button = screen.getByRole("button", {
      name: "Workspace: Data pipeline",
    });
    expect(button).toHaveAttribute("data-busy", "true");
  });

  it("lists a read-only share disabled, with view only as the reason", () => {
    show([sharedWorkspace("read", { name: "Q3 planning" })]);
    open();
    const item = screen.getByRole("menuitemradio", { name: /Q3 planning/ });
    expect(item).toBeDisabled();
    expect(item).toHaveTextContent("view only");
  });

  it("lists a fork still being copied disabled", () => {
    show([workspace({ name: "Copy", state: "forking" })]);
    open();
    expect(screen.getByRole("menuitemradio", { name: /Copy/ })).toBeDisabled();
  });

  it("adds a search field once there are more than 8, and filters by name", () => {
    const items = Array.from({ length: 9 }, (_, index) =>
      workspace({ name: index === 4 ? "Quarterly report" : `Notes ${index}` }),
    );
    show(items);
    open();
    const search = screen.getByRole("searchbox", {
      name: "Search workspaces",
    });
    expect(search).toHaveFocus();
    // Recent workspaces: at most 8 without a search.
    expect(screen.getAllByRole("menuitemradio")).toHaveLength(9);
    fireEvent.change(search, { target: { value: "quart" } });
    expect(
      screen.getAllByRole("menuitemradio").map((item) => item.textContent),
    ).toEqual(["New workspace", "Quarterly reportYou"]);
    fireEvent.change(search, { target: { value: "nothing" } });
    expect(screen.getByText("No workspace matches.")).toBeInTheDocument();
  });

  it("has no search field with 8 or fewer", () => {
    show(Array.from({ length: 8 }, () => workspace()));
    open();
    expect(screen.queryByRole("searchbox")).toBeNull();
  });
});
