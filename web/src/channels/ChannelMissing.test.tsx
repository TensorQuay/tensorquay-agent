import { fireEvent, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";

import { ChannelMissing } from "./ChannelMissing";

vi.mock("next/link", () => ({
  default: ({
    href,
    children,
    ...rest
  }: {
    href: string;
    children: ReactNode;
  } & Record<string, unknown>) => (
    <a href={href} {...rest}>
      {children}
    </a>
  ),
}));

const failure = (status: number) =>
  new ApiError(status, "not_found", "diagnostic");

// TEST: WEB-U-381
describe("a channel that couldn't be opened", () => {
  it.each([404, 403])(
    "with %i says it doesn't exist or can't be seen, and leads back to the channels",
    (status) => {
      const retry = vi.fn();
      render(<ChannelMissing error={failure(status)} retry={retry} />);
      expect(
        screen.getByRole("heading", { level: 1, name: "Channels" }),
      ).toBeVisible();
      expect(screen.getByRole("alert")).toHaveTextContent(
        "This channel doesn't exist, or you can't see it.",
      );
      expect(
        screen.getByRole("link", { name: "Go to channels" }),
      ).toHaveAttribute("href", "/channels");
      // Trying again wouldn't help.
      expect(
        screen.queryByRole("button", { name: "Try again" }),
      ).not.toBeInTheDocument();
    },
  );

  it("with any other failure says it couldn't be opened, and Try again asks again", () => {
    const retry = vi.fn();
    render(<ChannelMissing error={failure(503)} retry={retry} />);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "The channel could not be opened.",
    );
    expect(
      screen.queryByRole("link", { name: "Go to channels" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(retry).toHaveBeenCalledOnce();
  });
});
