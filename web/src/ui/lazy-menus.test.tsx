import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { Pencil, Trash2 } from "lucide-react";
import { afterEach, describe, expect, it, vi } from "vitest";

/** Fresh copies of the menus and their loader, with the open state's code arriving when `arrive` says so. */
async function cold(arrive: () => Promise<void> = () => Promise.resolve()) {
  vi.resetModules();
  const fetched = vi.fn();
  vi.doMock("./MenuSurfaces", async () => {
    fetched();
    await arrive();
    return vi.importActual("./MenuSurfaces");
  });
  return {
    fetched,
    ...(await import("./ActionMenu")),
    ...(await import("./MenuButton")),
    ...(await import("../auth/AccountMenu")),
    ...(await import("./lazy-menus")),
  };
}

afterEach(() => {
  vi.doUnmock("./MenuSurfaces");
  vi.resetModules();
});

const actions = [
  { id: "rename", label: "Rename", icon: Pencil, onSelect: () => undefined },
  { id: "delete", label: "Delete", icon: Trash2, onSelect: () => undefined },
];
const choices = [
  { id: "local", label: "Local model" },
  { id: "large", label: "Large model" },
];

// WEB-U-014's budget (the task page's first load), as WEB-U-176 for the files panel: what draws an open menu (the ⋯
// menus, the composer's +, a picker's list, the account menu) isn't part of the page's first load. Its code is fetched
// when a menu is about to open (its trigger under the pointer, pressed or focused), never because a page rendered, and a
// menu opened before it arrives opens when it does, focused as ever.
describe("WEB-U-014 a menu's open state loads when it is wanted", () => {
  it("is fetched on a trigger's pointer over, pointer down or focus, never on render", async () => {
    for (const touch of ["pointerEnter", "pointerDown", "focus"] as const) {
      const { fetched, ActionMenu } = await cold();
      const view = render(
        <ActionMenu label="More actions for Report" actions={actions} />,
      );
      expect(fetched).not.toHaveBeenCalled();
      const button = screen.getByRole("button", {
        name: "More actions for Report",
      });
      fireEvent[touch](
        touch === "focus" ? button : (button.parentElement as HTMLElement),
      );
      await vi.waitFor(() => expect(fetched).toHaveBeenCalledTimes(1));
      view.unmount();
    }
    const picker = await cold();
    render(
      <picker.MenuButton
        label="Model"
        options={choices}
        value="local"
        onChange={() => undefined}
      />,
    );
    expect(picker.fetched).not.toHaveBeenCalled();
    fireEvent.pointerEnter(
      screen.getByRole("button", { name: "Model: Local model" }),
    );
    await vi.waitFor(() => expect(picker.fetched).toHaveBeenCalledTimes(1));
    const account = await cold();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <account.AccountMenu />
      </QueryClientProvider>,
    );
    expect(account.fetched).not.toHaveBeenCalled();
    fireEvent.pointerEnter(screen.getByRole("button", { name: "Account" }));
    await vi.waitFor(() => expect(account.fetched).toHaveBeenCalledTimes(1));
  });

  it("opens a ⋯ menu clicked before its code arrived as soon as it does, its first item focused", async () => {
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = () => resolve();
    });
    const { ActionMenu } = await cold(() => gate);
    render(<ActionMenu label="More actions for Report" actions={actions} />);
    const button = screen.getByRole("button", {
      name: "More actions for Report",
    });
    fireEvent.click(button);
    expect(button).toHaveAttribute("aria-expanded", "true");
    expect(screen.queryByRole("menu")).toBeNull();
    release();
    const menu = await screen.findByRole("menu", {
      name: "More actions for Report",
    });
    expect(menu).toBeVisible();
    expect(screen.getByRole("menuitem", { name: "Rename" })).toHaveFocus();
  });

  it("opens a picker clicked before its code arrived as soon as it does, on its current choice", async () => {
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = () => resolve();
    });
    const { MenuButton } = await cold(() => gate);
    render(
      <MenuButton
        label="Model"
        options={choices}
        value="large"
        onChange={() => undefined}
      />,
    );
    const button = screen.getByRole("button", { name: "Model: Large model" });
    fireEvent.click(button);
    expect(button).toHaveAttribute("aria-expanded", "true");
    expect(button).not.toHaveAttribute("aria-controls");
    release();
    expect(
      await screen.findByRole("menuitemradio", { name: "Large model" }),
    ).toHaveFocus();
    expect(button).toHaveAttribute(
      "aria-controls",
      screen.getByRole("menu").id,
    );
  });

  it("closes a menu whose code didn't arrive, and tries again on the next opening", async () => {
    let tries = 0;
    const { ActionMenu } = await cold(() =>
      ++tries === 1 ? Promise.reject(new Error("offline")) : Promise.resolve(),
    );
    render(<ActionMenu label="More actions for Report" actions={actions} />);
    const button = screen.getByRole("button", {
      name: "More actions for Report",
    });
    fireEvent.click(button);
    await vi.waitFor(() =>
      expect(button).toHaveAttribute("aria-expanded", "false"),
    );
    fireEvent.click(button);
    expect(
      await screen.findByRole("menu", { name: "More actions for Report" }),
    ).toBeVisible();
    expect(tries).toBe(2);
  });
  // A menu waiting for its code is dismissed as an open one is (decided on 9 Nov 2026): Escape, a press outside its
  // trigger, or the focus leaving the trigger closes it, the trigger says so, and the menu never opens later.
  for (const [how, dismiss] of [
    [
      "Escape",
      (button: HTMLElement) => fireEvent.keyDown(button, { key: "Escape" }),
    ],
    ["a press outside", () => fireEvent.pointerDown(document.body)],
    [
      "the focus leaving its trigger",
      () =>
        act(() => {
          screen.getByRole("textbox", { name: "Note" }).focus();
        }),
    ],
  ] as const)
    it(`closes a menu waiting for its code on ${how}, and never opens it later`, async () => {
      let release: () => void = () => undefined;
      const gate = new Promise<void>((resolve) => {
        release = () => resolve();
      });
      const { ActionMenu, loadMenus } = await cold(() => gate);
      render(
        <>
          <ActionMenu label="More actions for Report" actions={actions} />
          <input aria-label="Note" />
        </>,
      );
      const button = screen.getByRole("button", {
        name: "More actions for Report",
      });
      act(() => button.focus());
      fireEvent.click(button);
      expect(button).toHaveAttribute("aria-expanded", "true");
      dismiss(button);
      expect(button).toHaveAttribute("aria-expanded", "false");
      const focused = document.activeElement;
      await act(async () => {
        release();
        await loadMenus();
      });
      expect(screen.queryByRole("menu")).toBeNull();
      expect(document.activeElement).toBe(focused);
    });
  // A picker that opened at once (New task's chip) and is dismissed while its code loads, or whose code can't come,
  // says so with onDismiss, as its open list does, so New task drops the empty chip (the review's second round).
  for (const [how, fails, dismiss] of [
    [
      "Escape",
      false,
      () => fireEvent.keyDown(document.body, { key: "Escape" }),
    ],
    ["a press outside", false, () => fireEvent.pointerDown(document.body)],
    ["a failed load", true, () => undefined],
  ] as const)
    it(`calls a picker's onDismiss once on ${how} before its code arrives`, async () => {
      let release: () => void = () => undefined;
      const gate = new Promise<void>((resolve) => {
        release = () => resolve();
      });
      const { MenuButton, loadMenus } = await cold(() =>
        fails ? Promise.reject(new Error("offline")) : gate,
      );
      const onDismiss = vi.fn();
      render(
        <MenuButton
          label="Workspace"
          options={choices}
          value=""
          placeholder="Choose a workspace"
          defaultOpen
          onChange={() => undefined}
          onDismiss={onDismiss}
        />,
      );
      dismiss();
      await act(async () => {
        release();
        await loadMenus().catch(() => undefined);
      });
      expect(onDismiss).toHaveBeenCalledTimes(1);
      expect(screen.queryByRole("menu")).toBeNull();
    });
});
