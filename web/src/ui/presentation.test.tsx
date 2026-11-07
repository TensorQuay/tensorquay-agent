import { render, screen } from "@testing-library/react";
import { Circle } from "lucide-react";
import { describe, expect, it } from "vitest";

import { Card } from "./Card";
import { EmptyState } from "./EmptyState";
import { Icon } from "./Icon";
import { Skeleton } from "./Skeleton";
import { StatusPill } from "./StatusPill";

describe("presentation primitives", () => {
  // ADR-0035: no approval statuses (waiting for approval, denied); a called command is starting.
  it.each([
    "running",
    "starting",
    "queued",
    "sleeping",
    "idle",
    "completed",
    "failed",
  ] as const)("names the %s status", (status) => {
    render(<StatusPill status={status} />);
    expect(screen.getByText(new RegExp(status, "i"))).toBeInTheDocument();
  });
  it("renders card content", () => {
    render(<Card>Workspace details</Card>);
    expect(screen.getByText("Workspace details")).toBeInTheDocument();
  });
  // The design direction: an icon that names the thing (never the logo), one sentence, one primary action.
  it("renders an empty state as the thing's icon, a sentence and one action", () => {
    const { container } = render(
      <EmptyState
        icon={Circle}
        message="No items yet."
        action={<button>Start</button>}
      />,
    );
    expect(screen.getByText("No items yet.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start" })).toBeInTheDocument();
    const icon = container.querySelector(".empty-state svg");
    expect(icon).toHaveAttribute("aria-hidden", "true");
    expect(icon).toHaveAttribute("width", "20");
    expect(container.querySelector(".brand-mark")).toBeNull();
  });
  it("announces loading without a page spinner", () => {
    render(<Skeleton label="Loading tasks" />);
    expect(screen.getByRole("status")).toHaveAccessibleName("Loading tasks");
  });
  it.each([16, 20] as const)("renders a decorative icon at %s", (size) => {
    const { container } = render(<Icon glyph={Circle} size={size} />);
    const icon = container.querySelector("svg");
    expect(icon).toHaveAttribute("width", String(size));
    expect(icon).toHaveAttribute("stroke-width", "1.5"); // UI-GUIDELINES §5b: one 1.5 px stroke
    expect(icon).toHaveAttribute("aria-hidden", "true");
  });
});

// UI-GUIDELINES §7: menus anchored at the bottom of the sidebar open upward.
describe("popover placement", () => {
  it("places an upward popover above its trigger, aligned to its start", async () => {
    const { Popover } = await import("./Popover");
    const { createRef } = await import("react");
    const anchor = createRef<HTMLButtonElement>();
    render(
      <>
        <button ref={anchor}>Account</button>
        <Popover
          anchor={anchor}
          label="Account actions"
          placement="above-start"
          onClose={() => undefined}
        >
          <p>Menu</p>
        </Popover>
      </>,
    );
    const panel = screen
      .getByText("Menu")
      .closest("[aria-label='Account actions']");
    if (!(panel instanceof HTMLElement) || !anchor.current)
      throw new Error("Popover must render");
    anchor.current.getBoundingClientRect = () =>
      ({
        top: 800,
        bottom: 836,
        left: 12,
        right: 248,
        width: 236,
        height: 36,
      }) as DOMRect;
    Object.defineProperty(panel, "offsetHeight", {
      value: 200,
      configurable: true,
    });
    Object.defineProperty(panel, "offsetWidth", { value: 220 });
    window.dispatchEvent(new Event("resize"));
    // The bottom edge is pinned just above the trigger, so content that arrives later grows the menu upward without a
    // second placement (WEB-E2E-018: a lazily loaded menu briefly covered its trigger).
    expect(panel.style.bottom).toBe(`${innerHeight - 800}px`);
    expect(panel.style.top).toBe("auto");
    expect(panel.style.left).toBe("12px");
    Object.defineProperty(panel, "offsetHeight", { value: 487 });
    expect(panel.style.bottom).toBe(`${innerHeight - 800}px`);
    expect(panel.style.top).toBe("auto");
  });

  // The UI standard: never over its trigger. Taller than the room above and fitting below, it flips under the trigger.
  it("flips an upward popover taller than the space above to below its trigger", async () => {
    const { Popover } = await import("./Popover");
    const { createRef } = await import("react");
    const anchor = createRef<HTMLButtonElement>();
    render(
      <>
        <button ref={anchor}>Account</button>
        <Popover
          anchor={anchor}
          label="Account actions"
          placement="above-start"
          onClose={() => undefined}
        >
          <p>Menu</p>
        </Popover>
      </>,
    );
    const panel = screen
      .getByText("Menu")
      .closest("[aria-label='Account actions']");
    if (!(panel instanceof HTMLElement) || !anchor.current)
      throw new Error("Popover must render");
    anchor.current.getBoundingClientRect = () =>
      ({
        top: 300,
        bottom: 336,
        left: 12,
        right: 248,
        width: 236,
        height: 36,
      }) as DOMRect;
    Object.defineProperty(panel, "offsetHeight", { value: 400 });
    window.dispatchEvent(new Event("resize"));
    expect(panel.style.top).toBe("336px");
    expect(panel.style.bottom).toBe("auto");
  });
});
