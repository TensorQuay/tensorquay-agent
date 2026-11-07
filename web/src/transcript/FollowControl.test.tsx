import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { FollowControl } from "./FollowControl";

describe("WEB-U-015 jump control copy", () => {
  it.each([
    [0, "Jump to latest"],
    [1, "Jump to latest · 1 new"],
    [3, "Jump to latest · 3 new"],
  ] as const)(
    "labels %i unseen updates without a zero count",
    (unseen, label) => {
      const resume = vi.fn();
      render(<FollowControl unseen={unseen} onResume={resume} />);
      fireEvent.click(screen.getByRole("button", { name: label }));
      expect(resume).toHaveBeenCalledOnce();
      expect(screen.queryByText(/0 new/)).toBeNull();
    },
  );
});
