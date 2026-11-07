import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ApiError } from "../api/error";

import { ErrorMessage } from "./ErrorMessage";

// UI-GUIDELINES §14 / §11b: the web owns the words - an error's stable code selects translated copy.
describe("WEB-U-019 error copy by code", () => {
  it.each([
    ["network", "The server could not be reached. Please try again."],
    ["timeout", "The request timed out. Please try again."],
    [
      "invalid_response",
      "The server response could not be read. Please try again.",
    ],
    ["rate_limited", "Too many attempts."],
    // GW-I-147 (503, retryable): no key is used without its audit row.
    [
      "key_use_unrecorded",
      "The key could not be used because its use could not be recorded. Try again in a moment; if it keeps happening, tell an administrator.",
    ],
    ["something_new", "Something went wrong. Please try again."],
  ])("shows %s as web copy, never the gateway message", (code, text) => {
    render(
      <ErrorMessage error={new ApiError(400, code, "Private diagnostic")} />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(text);
    expect(screen.queryByText(/Private diagnostic/)).toBeNull();
  });

  // Finding 3: a server error whose body isn't JSON read as "could not be read", which misleads.
  it.each([500, 502, 503])(
    "says a %s is the server's problem, whatever its body",
    (status) => {
      render(
        <ErrorMessage
          error={new ApiError(status, "invalid_response", "unreadable")}
        />,
      );
      expect(screen.getByRole("alert")).toHaveTextContent(
        "The server had a problem. Please try again.",
      );
    },
  );

  it("adds the retry delay and passes through local copy", () => {
    const { rerender } = render(
      <ErrorMessage error={new ApiError(429, "rate_limited", "x", 11.2)} />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Too many attempts. Try again in 12 s.",
    );
    rerender(
      <ErrorMessage error="Environments could not be loaded. Try again." />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Environments could not be loaded.",
    );
  });
});
