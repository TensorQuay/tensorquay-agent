import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { Input } from "../ui/Input";

import { PageSection, SettingsPage } from "./SettingsPage";

const push = vi.fn();
vi.mock("next/navigation", () => ({ useRouter: () => ({ push }) }));

function show(
  props: Partial<Parameters<typeof SettingsPage>[0]> = {},
  elsewhere = true,
) {
  const onSave = vi.fn();
  render(
    <>
      {/* A link elsewhere on the page, as the Settings menu or the sidebar is. */}
      {elsewhere && <a href="/help/models">Models</a>}
      <SettingsPage
        back={{ href: "/settings/environments", label: "Environments" }}
        title="Workstation"
        dirty={false}
        pending={false}
        failure={null}
        onSave={onSave}
        {...props}
      >
        <PageSection title="Basics">
          <Input label="Name" defaultValue="workstation" />
        </PageSection>
      </SettingsPage>
    </>,
  );
  return { onSave };
}

beforeEach(() => push.mockReset());

// WEB-U-090 (the UI standard's Page): a long form is a page of its own, with the way back at the top, its sections, and
// Save and Cancel pinned at the bottom; leaving with unsaved changes asks first, whichever way the person leaves.
describe("WEB-U-090 the Settings page frame", () => {
  it("has the way back, the title, its sections and its actions", () => {
    show();
    expect(screen.getByRole("link", { name: "Environments" })).toHaveAttribute(
      "href",
      "/settings/environments",
    );
    expect(
      screen.getByRole("heading", { level: 2, name: "Workstation" }),
    ).toBeVisible();
    expect(screen.getByRole("region", { name: "Basics" })).toContainElement(
      screen.getByLabelText("Name"),
    );
    const actions = screen.getByRole("group", { name: "Save or cancel" });
    expect(within(actions).getByRole("button", { name: "Save" })).toBeVisible();
    expect(
      within(actions).getByRole("button", { name: "Cancel" }),
    ).toBeVisible();
    expect(screen.queryByText("Unsaved changes")).toBeNull();
  });

  it("saves on Save and on Enter in a field, and shows the save pending", () => {
    const { onSave } = show({ dirty: true });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onSave).toHaveBeenCalledOnce();
    fireEvent.submit(screen.getByLabelText("Name"));
    expect(onSave).toHaveBeenCalledTimes(2);
  });

  it("says a failure in plain words beside Save", () => {
    show({
      dirty: true,
      failure: "The environment couldn't be saved. Try again.",
    });
    expect(
      within(screen.getByRole("group", { name: "Save or cancel" })).getByRole(
        "alert",
      ),
    ).toHaveTextContent("The environment couldn't be saved. Try again.");
  });

  it("offers only Close on a page that can't be changed now", () => {
    const { onSave } = show({ readOnly: true });
    const actions = screen.getByRole("group", { name: "Save or cancel" });
    expect(within(actions).queryByRole("button", { name: "Save" })).toBeNull();
    fireEvent.submit(screen.getByLabelText("Name"));
    expect(onSave).not.toHaveBeenCalled();
    fireEvent.click(within(actions).getByRole("button", { name: "Close" }));
    expect(push).toHaveBeenCalledExactlyOnceWith("/settings/environments");
  });

  it("goes back at once on Cancel when nothing changed", () => {
    show();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(push).toHaveBeenCalledExactlyOnceWith("/settings/environments");
  });

  it("says there are unsaved changes, and asks before Cancel leaves them", () => {
    show({ dirty: true });
    // A status line, beside the frame's other one, which counts fields to fix after a refused save (WEB-U-110).
    expect(screen.getByText("Unsaved changes")).toHaveAttribute(
      "role",
      "status",
    );
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    const ask = screen.getByRole("dialog", { name: "Leave without saving?" });
    fireEvent.click(within(ask).getByRole("button", { name: "Keep editing" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(push).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(
      within(
        screen.getByRole("dialog", { name: "Leave without saving?" }),
      ).getByRole("button", { name: "Leave" }),
    );
    expect(push).toHaveBeenCalledExactlyOnceWith("/settings/environments");
  });

  it.each([
    ["Models", "/help/models"],
    ["Environments", "/settings/environments"],
  ] as const)(
    "asks before the link %s leaves unsaved changes, then goes on Leave",
    (name, href) => {
      show({ dirty: true });
      const link = screen.getByRole("link", { name });
      // A link's own click is cancelled while the page asks.
      expect(fireEvent.click(link)).toBe(false);
      expect(push).not.toHaveBeenCalled();
      fireEvent.click(
        within(
          screen.getByRole("dialog", { name: "Leave without saving?" }),
        ).getByRole("button", { name: "Leave" }),
      );
      expect(push).toHaveBeenCalledExactlyOnceWith(href);
    },
  );

  it("lets links go when nothing changed, and a link opened elsewhere always", () => {
    show();
    expect(fireEvent.click(screen.getByRole("link", { name: "Models" }))).toBe(
      true,
    );
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("asks the browser to confirm closing or reloading with unsaved changes", () => {
    show({ dirty: true });
    const event = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
  });

  it("lets the browser close the page when nothing changed", () => {
    show();
    const event = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  });
});
