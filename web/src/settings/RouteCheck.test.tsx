import { readFileSync } from "node:fs";

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiClient } from "../api/client";
import { ApiError } from "../api/error";
import { fakeRoutes, probeResult, route } from "../test/routes";

import { gatewayRoutesFor } from "./admin/gateway";
import type { ProbeResult, RoutesApi } from "./admin/routes";
import { FAILURES } from "./probe-copy";
import { RouteCheck } from "./RouteCheck";

function show(probes = {}, extra = {}, local = false) {
  const fake = fakeRoutes({ routes: [route("local", extra)], probes });
  const onClose = vi.fn();
  render(
    <RouteCheck
      name="local"
      local={local}
      api={fake.api}
      onClose={onClose}
      onSuggest={vi.fn()}
    />,
  );
  return {
    fake,
    onClose,
    dialog: screen.getByRole("dialog", { name: "Check route local" }),
  };
}
const run = (dialog: HTMLElement) =>
  fireEvent.click(within(dialog).getByRole("button", { name: "Run check" }));

// A route check runs only when asked, and reports what it observed in plain words (WEB-U-039).
describe("WEB-U-039 checking a route", () => {
  it("says what a check does, runs it once on request, and reports a verified route", async () => {
    const { fake, dialog } = show();
    expect(dialog).toHaveTextContent(
      "uses a few tokens. It can take up to a minute.",
    );
    expect(fake.writes).toEqual([]);
    run(dialog);
    const result = await within(dialog).findByRole("region", {
      name: "Check result",
    });
    expect(result).toHaveTextContent("Verified: this route works for tasks.");
    // The verdict first; what was observed is folded under Details (F7), and the button offers to run it again.
    const details = within(result).getByText("Details").closest("details");
    expect(details).not.toHaveAttribute("open");
    expect(
      within(dialog).getByRole("button", { name: "Run again" }),
    ).toBeVisible();
    expect(
      within(dialog).queryByRole("button", { name: "Run check" }),
    ).toBeNull();
    fireEvent.click(within(result).getByText("Details"));
    for (const [term, detail] of [
      ["Streaming", "Yes"],
      ["Tool call", "Passed"],
      ["Tool result round trip", "Passed"],
      ["First response", "420 ms"],
      ["Tokens used", "212 in, 38 out"],
    ] as const)
      expect(
        within(result).getByText(term).nextElementSibling,
      ).toHaveTextContent(detail);
    expect(fake.writes).toHaveLength(1);
    expect(fake.writes[0]?.key).toMatch(/.+/);
  });

  it("explains a failed check with the server's answer as plain text, and its warnings", async () => {
    const excerpt = '<img src=x onerror=alert(1)> {"error":"invalid key"}';
    const failed = probeResult("failed", {
      ok: false,
      tool_call: "not_run",
      round_trip: "not_run",
      failure: { code: "auth_rejected", status: 401, excerpt },
      warnings: ["usage_missing"],
    });
    const { dialog } = show({ local: failed });
    run(dialog);
    const result = await within(dialog).findByRole("region", {
      name: "Check result",
    });
    expect(result).toHaveTextContent(
      "The check failed. The server refused the route’s key.",
    );
    expect(within(result).getByText(excerpt)).toBeVisible();
    expect(result.querySelector("img")).toBeNull();
    expect(result).toHaveTextContent("Status 401");
    expect(result).toHaveTextContent("The server did not report token usage");
    // WEB-U-264: the next step for a timeout isn't said for another failure.
    expect(result).not.toHaveTextContent(
      /Try again when it's idle|Try again in a moment/,
    );
  });

  it("says why a check cannot run, with a way forward", async () => {
    const { dialog } = show({}, { key_missing: true });
    run(dialog);
    expect(
      await within(dialog).findByText("This route has no key yet."),
    ).toBeVisible();
    expect(
      within(dialog).getByRole("link", { name: "Add it under Keys › Shared" }),
    ).toHaveAttribute("href", "/settings/keys/shared");
  });

  it("gives the wait when the route was checked too recently", async () => {
    const { dialog } = show({
      local: new ApiError(429, "rate_limited", "x", 17),
    });
    run(dialog);
    expect(
      await within(dialog).findByText(
        "Checked too recently. Try again in 17 s.",
      ),
    ).toBeVisible();
  });
});

// GW-U-064: the check tries a two-line value through strict tools, since some servers drop its line break in strict
// mode only; the step reads as its own row (WEB-U-061).
describe("WEB-U-061 the strict line breaks step", () => {
  it.each([
    ["passed", "Passed"],
    ["failed", "Failed"],
    ["not_declared", "Not declared"],
    ["not_run", "Not run"],
  ] as const)("shows %s as its own row", async (outcome, words) => {
    const { dialog } = show({
      local: probeResult(outcome === "failed" ? "failed" : "verified", {
        strict_line_breaks: outcome,
        ...(outcome === "failed"
          ? {
              ok: false,
              failure: {
                code: "strict_line_breaks_lost",
                status: null,
                excerpt: null,
              },
            }
          : {}),
      }),
    });
    run(dialog);
    const result = await within(dialog).findByRole("region", {
      name: "Check result",
    });
    expect(
      within(result).getByText("Strict tools keep line breaks")
        .nextElementSibling,
    ).toHaveTextContent(words);
    if (outcome === "failed")
      expect(result).toHaveTextContent(
        "With strict tools the server dropped the line break from a tool’s text",
      );
  });

  it("leaves the row out for a gateway that does not report the step", async () => {
    const { dialog } = show();
    run(dialog);
    const result = await within(dialog).findByRole("region", {
      name: "Check result",
    });
    expect(
      within(result).queryByText("Strict tools keep line breaks"),
    ).toBeNull();
  });
});

// Every failure a person can meet in the check says so in plain words, never as a code (the team's rule after a raw
// code reached the owner): each failure the probe reports, a code this page doesn't know yet, and each refusal.
const english = (
  JSON.parse(readFileSync("src/i18n/locales/en.json", "utf8")) as {
    messages: Record<string, string>;
  }
).messages;
describe("WEB-U-039 every check failure in plain words", () => {
  const failed = (code: string) =>
    probeResult("failed", {
      ok: false,
      tool_call: "not_run",
      round_trip: "not_run",
      failure: { code, status: null, excerpt: null },
    } as Parameters<typeof probeResult>[1]);

  it.each([
    ...Object.entries(FAILURES),
    ["from_a_newer_gateway", "models.failUnknown"],
  ])("words the failure %s", async (code, key) => {
    const { dialog } = show({ local: failed(code) });
    run(dialog);
    const result = await within(dialog).findByRole("region", {
      name: "Check result",
    });
    expect(english[key]).toBeTruthy();
    expect(result).toHaveTextContent(`The check failed. ${english[key]}`);
    // A code-shaped word (snake_case) never shows; a plain word like "cancelled" may be in its own sentence.
    if (code.includes("_")) expect(result.textContent).not.toContain(code);
  });

  it.each([
    [new ApiError(409, "key_missing", "x"), "This route has no key yet."],
    [
      new ApiError(409, "probe_running", "x"),
      "A check of this route is already running.",
    ],
    [
      new ApiError(429, "rate_limited", "x", 12),
      "Checked too recently. Try again in 12 s.",
    ],
    [
      new ApiError(429, "rate_limited", "x"),
      "Checked too recently. Try again shortly.",
    ],
    [
      new ApiError(504, "timeout", "x"),
      "The check ran out of time and stopped. Local models can take a minute or two to answer: try again once the model has loaded.",
    ],
    [new ApiError(404, "not_found", "x"), "This route no longer exists."],
    [new ApiError(500, "internal", "x"), "The check could not run."],
  ])("words the refusal %s", async (error, copy) => {
    const { dialog } = show({ local: error });
    run(dialog);
    expect(await within(dialog).findByText(copy)).toBeVisible();
    if (error.code.includes("_"))
      expect(dialog.textContent).not.toContain(error.code);
  });
});

// WEB-U-095: a model on this computer (or one without a key) can take a while to answer, and its check gets longer
// (GW-U-092), so the dialog says so before and while it runs.
describe("WEB-U-095 checking a local route", () => {
  const intro =
    "Sends a short request with one tool call through this route, as a task would, and uses a few tokens.";

  it("says a local model can take a minute or two, before and while it checks", () => {
    const { dialog } = show({}, {}, true);
    expect(dialog).toHaveTextContent(
      `${intro} Local models can take a minute or two.`,
    );
    run(dialog);
    expect(within(dialog).getByRole("status")).toHaveTextContent(
      "Checking… local models can take a minute or two.",
    );
  });

  it("keeps the minute for a route elsewhere", () => {
    const { dialog } = show();
    expect(dialog).toHaveTextContent(`${intro} It can take up to a minute.`);
    run(dialog);
    expect(within(dialog).getByRole("status")).toHaveTextContent(/^Checking…$/);
  });
});

// A first-hour check before 17.0: the README's default small model on a CPU-only computer works, slowly, and its check
// timed out. The check now waits while the model keeps answering, and says how slow it is: verified, but slowly.
describe("WEB-U-095 a local model that answers slowly", () => {
  const slow = probeResult("verified", {
    warnings: ["thinks_unasked", "slow_answers"],
    output_tokens_per_second: 7,
  });
  const words =
    "Verified, but slowly: this model answers at about 7 tokens a second here, so a task's step can take minutes, and helpers or a Desktop task far longer. A computer with a suitable GPU, or a hosted model, is much faster. See the user guide's Running without a GPU section.";

  it("says it works but slowly, with its speed, once, and announces it", async () => {
    const { dialog } = show({ local: slow }, {}, true);
    run(dialog);
    const result = await within(dialog).findByRole("region", {
      name: "Check result",
    });
    expect(within(result).getByText(words)).toBeVisible();
    expect(result).not.toHaveTextContent(
      "Verified: this route works for tasks.",
    );
    // The verdict says it; no note repeats it.
    expect(within(result).queryAllByRole("note")).toHaveLength(0);
    expect(within(dialog).getByRole("status")).toHaveTextContent(words);
    // The speed is in the details too.
    expect(result).toHaveTextContent("Output speed7 tokens a second");
  });

  it("keeps the plain verdict without a speed to give", async () => {
    const { dialog } = show(
      { local: probeResult("verified", { warnings: ["slow_answers"] }) },
      {},
      true,
    );
    run(dialog);
    const result = await within(dialog).findByRole("region", {
      name: "Check result",
    });
    expect(result).toHaveTextContent("Verified: this route works for tasks.");
  });

  it("says how long it has waited after the first minute, on a local route only", async () => {
    vi.useFakeTimers();
    try {
      const waiting = {
        probe: () => new Promise(() => undefined),
      } as unknown as RoutesApi;
      for (const local of [true, false]) {
        render(
          <RouteCheck
            name="local"
            local={local}
            api={waiting}
            onClose={vi.fn()}
            onSuggest={vi.fn()}
          />,
        );
        const dialog = screen.getByRole("dialog", {
          name: "Check route local",
        });
        run(dialog);
        await act(() => vi.advanceTimersByTimeAsync(30_000));
        expect(dialog).not.toHaveTextContent("Still checking");
        await act(() => vi.advanceTimersByTimeAsync(105_000));
        if (local)
          expect(dialog).toHaveTextContent(
            "Still checking: 2 minutes so far. A local model on a computer without a suitable GPU can take several minutes; the check waits as long as it keeps answering.",
          );
        else expect(dialog).not.toHaveTextContent("Still checking");
        cleanup();
      }
    } finally {
      vi.useRealTimers();
    }
  });
});

// WEB-U-271: the "Checking…" status went away when the result came, so a screen reader heard the start and never the
// outcome. One status line stays: running, then the verdict, or why the check couldn't run; the visible "Checking…"
// isn't read twice.
describe("WEB-U-271 a route check's outcome is announced", () => {
  it("says it is checking, then the verdict, on the same status line", async () => {
    const failed = probeResult("failed", {
      ok: false,
      tool_call: "not_run",
      round_trip: "not_run",
      failure: { code: "timeout", status: null, excerpt: null },
    });
    const { dialog } = show({ local: failed });
    const status = within(dialog).getByRole("status");
    expect(status).toHaveTextContent(/^$/);
    run(dialog);
    expect(status).toHaveTextContent(/^Checking…$/);
    await within(dialog).findByRole("region", { name: "Check result" });
    expect(within(dialog).getByRole("status")).toBe(status);
    expect(status).toHaveTextContent(
      "Check finished: The check failed. The server did not answer in time.",
    );
  });

  it("says why a check couldn't run", async () => {
    const { dialog } = show({}, { key_missing: true });
    run(dialog);
    await within(dialog).findByText("This route has no key yet.");
    expect(within(dialog).getByRole("status")).toHaveTextContent(
      "Check not run: This route has no key yet.",
    );
  });
});

// WEB-U-133 (F16): a model that thinks before every answer needs room, never a switch that can't turn it off; the advice
// shows only when a set Max output is below 16384 (4096 was the room candidate 12's qwen3:4b spent thinking on step 1,
// CORE-U-072), and the one click that fills Max output in on the route's form, unsaved, only when what it offers is
// above that limit: 16384, or half a smaller Context window, which Max output must fit.
describe("WEB-U-133 a model that thinks unasked", () => {
  const thinks = probeResult("verified", { warnings: ["thinks_unasked"] });
  const words =
    "This model thinks before every answer, so it needs room: with a low Max output it can spend it all thinking and write no answer.";
  async function note(
    maxOutput: number | null,
    contextWindow: number | null = null,
  ) {
    const fake = fakeRoutes({
      routes: [route("local")],
      probes: { local: thinks },
    });
    const onSuggest = vi.fn();
    render(
      <RouteCheck
        name="local"
        maxOutput={maxOutput}
        contextWindow={contextWindow}
        api={fake.api}
        onClose={vi.fn()}
        onSuggest={onSuggest}
      />,
    );
    const dialog = screen.getByRole("dialog", { name: "Check route local" });
    run(dialog);
    const result = await within(dialog).findByRole("region", {
      name: "Check result",
    });
    return { result, onSuggest };
  }

  it.each([2048, 4096, 8192])(
    "says it needs room and offers Max output 16384 when %s is set",
    async (maxOutput) => {
      const { result, onSuggest } = await note(maxOutput);
      expect(within(result).getByRole("note")).toHaveTextContent(words);
      // Never a number that goes stale, never a blank limit: that lets a runaway thinker run to the context window.
      expect(within(result).getByRole("note")).not.toHaveTextContent(
        /\d|blank|Reasoning/,
      );
      fireEvent.click(
        within(result).getByRole("button", {
          name: /^Set Max output to 16\s384$/,
        }),
      );
      expect(onSuggest).toHaveBeenCalledExactlyOnceWith({
        field: "max_output",
        value: "16384",
      });
    },
  );

  it("offers half a smaller Context window, which Max output must fit", async () => {
    const { result, onSuggest } = await note(2048, 8192);
    expect(within(result).getByRole("note")).toHaveTextContent(words);
    fireEvent.click(
      within(result).getByRole("button", {
        name: /^Set Max output to 4\s096$/,
      }),
    );
    expect(onSuggest).toHaveBeenCalledExactlyOnceWith({
      field: "max_output",
      value: "4096",
    });
  });

  it("says it needs room but offers nothing no higher than the limit set", async () => {
    const { result } = await note(4096, 8192);
    expect(within(result).getByRole("note")).toHaveTextContent(words);
    expect(
      within(result).queryByRole("button", { name: /Set Max output/ }),
    ).toBeNull();
  });

  // A pass over the pages before 17.0: with Max output blank or already 16384 or more there is nothing to do, so
  // nothing reads like a task: no advice, no action.
  it.each([null, 16384, 32768])(
    "says nothing and offers no action when Max output is %s",
    async (maxOutput) => {
      const { result } = await note(maxOutput);
      expect(
        within(result).queryByText(/thinks before every answer/),
      ).toBeNull();
      expect(
        within(result).queryByRole("button", { name: /Set Max output/ }),
      ).toBeNull();
    },
  );
});

// WEB-U-138 (the c13c how-to check): a local model's check can take a minute or two, and the gateway bounds it itself;
// the page waits for it instead of giving up at the web's usual 10 s ("The check could not run.").
describe("WEB-U-138 a slow check", () => {
  it("shows the result of a check that answers at 12 s", async () => {
    vi.useFakeTimers();
    try {
      const fetch = vi.fn<typeof globalThis.fetch>(
        (_url, init) =>
          new Promise<Response>((resolve, reject) => {
            const timer = setTimeout(
              () => resolve(Response.json({ probe: probeResult("verified") })),
              12_000,
            );
            init?.signal?.addEventListener("abort", () => {
              clearTimeout(timer);
              reject(new DOMException("aborted", "AbortError"));
            });
          }),
      );
      const api = gatewayRoutesFor(
        new ApiClient({
          fetch,
          redirect: vi.fn(),
          sleep: vi.fn().mockResolvedValue(undefined),
          currentPath: () => "/settings/models",
        }),
      );
      render(
        <RouteCheck
          name="local"
          local
          api={api}
          onClose={vi.fn()}
          onSuggest={vi.fn()}
        />,
      );
      const dialog = screen.getByRole("dialog", { name: "Check route local" });
      run(dialog);
      // The answer arrives at 12 s; the page then renders it (a tick later under fake timers).
      await vi.advanceTimersByTimeAsync(12_000);
      await vi.advanceTimersByTimeAsync(0);
      expect(
        within(dialog).getByRole("region", { name: "Check result" }),
      ).toHaveTextContent("Verified: this route works for tasks.");
      expect(dialog).not.toHaveTextContent("The check could not run.");
    } finally {
      vi.useRealTimers();
    }
  });
});

// WEB-U-264 (a first-hour check before 17.0): a check that timed out said only "The server did not answer in time". It
// now says what to do next: for a local model, that it may be busy or slow here, to try again when it is idle, and the
// setting an administrator can raise; for another route, to try again in a moment. Another failure gets neither line.
describe("WEB-U-264 a check that timed out says what to do next", () => {
  const timedOut = probeResult("failed", {
    ok: false,
    tool_call: "not_run",
    round_trip: "not_run",
    failure: { code: "timeout", status: null, excerpt: null },
    warnings: [],
  });
  const local =
    "A local model may be busy with another task or slow on this computer. Try again when it's idle; you can give local checks longer with [relay] local_check_timeout in gateway.toml.";
  const hosted = "The server may be busy. Try again in a moment.";

  it("says a local model may be busy, and how an administrator gives it longer", async () => {
    const { dialog } = show({ local: timedOut }, {}, true);
    run(dialog);
    const result = await within(dialog).findByRole("region", {
      name: "Check result",
    });
    expect(result).toHaveTextContent(
      "The check failed. The server did not answer in time.",
    );
    expect(within(result).getByText(local)).toBeVisible();
    expect(within(result).queryByText(hosted)).toBeNull();
  });

  it("says another server may be busy", async () => {
    const { dialog } = show({ local: timedOut });
    run(dialog);
    const result = await within(dialog).findByRole("region", {
      name: "Check result",
    });
    expect(within(result).getByText(hosted)).toBeVisible();
    expect(within(result).queryByText(local)).toBeNull();
  });
});

// WEB-U-269 (a first-hour check before 17.0): a local Ollama with Ollama's default 4,096-token context truncates the
// prompt silently. Under 16 384 the check fails naming both sizes and how to give Ollama more room, on a Mac, on Linux
// and in Docker, and offers to set the route's Context window to the size Ollama gives; at 16 384 or more, a route
// whose Context window isn't that size is offered the same one click. Numbers group with a narrow no-break space
// (UI-GUIDELINES §10), which the testing library reads as a space.
describe("WEB-U-269 a local model's context", () => {
  const server = {
    server: "ollama",
    context_length: 4096,
    minimum: 16384,
  } as const;
  function check(
    result: ReturnType<typeof probeResult>,
    contextWindow: number | null = null,
  ) {
    const fake = fakeRoutes({
      routes: [route("local")],
      probes: { local: result },
    });
    const onSuggest = vi.fn();
    render(
      <RouteCheck
        name="local"
        local
        contextWindow={contextWindow}
        api={fake.api}
        onClose={vi.fn()}
        onSuggest={onSuggest}
      />,
    );
    const dialog = screen.getByRole("dialog", { name: "Check route local" });
    run(dialog);
    return { dialog, onSuggest };
  }

  it("fails a context too small, with both sizes, the fix on each platform and one click", async () => {
    const { dialog, onSuggest } = check(
      probeResult("failed", {
        ok: false,
        failure: {
          code: "context_too_small",
          status: null,
          excerpt: null,
          suggestion: {
            field: "context_window",
            value: "4096",
            reason: "the server's context is under the minimum",
          },
        },
        server_context: server,
      }),
    );
    const result = await within(dialog).findByRole("region", {
      name: "Check result",
    });
    expect(result).toHaveTextContent(
      "The check failed. Ollama gives this model 4 096 tokens of context; tasks need at least 16 384.",
    );
    expect(
      within(result).getByText(
        /^On a Mac: in Terminal, run launchctl setenv OLLAMA_CONTEXT_LENGTH 32768/,
      ),
    ).toBeVisible();
    expect(
      within(result).getByText(/^On Linux: run systemctl edit ollama\.service/),
    ).toBeVisible();
    expect(
      within(result).getByText(
        "In Docker: start the Ollama container with -e OLLAMA_CONTEXT_LENGTH=32768.",
      ),
    ).toBeVisible();
    expect(
      within(result).getByText(
        "ollama ps shows the size each loaded model got, in its CONTEXT column.",
      ),
    ).toBeVisible();
    expect(
      within(result).getByText(
        /^On Windows: quit Ollama, add the user environment variable OLLAMA_CONTEXT_LENGTH/,
      ),
    ).toBeVisible();
    // WEB-U-290: below the minimum, matching the route to the server's size wouldn't make tasks work: not offered.
    expect(
      within(result).queryByRole("button", { name: /^Set Context window/ }),
    ).toBeNull();
    expect(within(result).queryByText(/^Meanwhile/)).toBeNull();
    expect(onSuggest).not.toHaveBeenCalled();
  });

  it("says the context is too small in general words when the check couldn't read the sizes", async () => {
    const { dialog } = check(
      probeResult("failed", {
        ok: false,
        failure: { code: "context_too_small", status: null, excerpt: null },
      }),
    );
    const result = await within(dialog).findByRole("region", {
      name: "Check result",
    });
    expect(result).toHaveTextContent(
      "The check failed. The model server's context is too small for tasks.",
    );
  });

  it("offers the server's size when the route's Context window differs or is empty", async () => {
    const differs = probeResult("verified", {
      warnings: ["context_window_differs"],
      server_context: { ...server, context_length: 32768 },
    });
    const { dialog, onSuggest } = check(differs, 8192);
    expect(
      await within(dialog).findByText(
        "Ollama gives this model 32 768 tokens of context, but this route's Context window says 8 192.",
      ),
    ).toBeVisible();
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Set Context window to 32\u202f768",
      }),
    );
    expect(onSuggest).toHaveBeenCalledWith({
      field: "context_window",
      value: "32768",
    });
    cleanup();
    const unset = check(differs, null);
    expect(
      await within(unset.dialog).findByText(
        /^Ollama gives this model 32.768 tokens of context\. Set this route's Context window to match/,
      ),
    ).toBeVisible();
  });
});

// WEB-U-298 (an accessibility review before 17.0): a failed check's next step is heard with its verdict, in the
// result's own words, so a screen-reader user isn't left to read on to find it; a failure with no next step announces
// the verdict alone. And a context suggestion arriving with another failure shows nothing, never the generic
// tool-settings line.
type Failure = NonNullable<ProbeResult["observed"]["failure"]>;

describe("WEB-U-298 a check's announcement carries its next step", () => {
  const announce = async (
    failure: Pick<Failure, "code"> & Partial<Failure>,
    props: { address?: string; protocol?: "openai_chat"; local?: boolean },
  ) => {
    const fake = fakeRoutes({
      routes: [route("local")],
      probes: {
        local: probeResult("failed", {
          ok: false,
          failure: { status: null, excerpt: null, ...failure },
        }),
      },
    });
    render(
      <RouteCheck
        name="local"
        local={props.local ?? false}
        address={props.address ?? ""}
        protocol={props.protocol}
        api={fake.api}
        onClose={vi.fn()}
        onSuggest={vi.fn()}
      />,
    );
    const dialog = screen.getByRole("dialog", { name: "Check route local" });
    run(dialog);
    await within(dialog).findByRole("region", { name: "Check result" });
    return within(dialog)
      .getAllByRole("status")
      .map((line) => line.textContent)
      .find((text) => text?.startsWith("Check finished:"));
  };

  it.each([
    [
      "a localhost route the gateway reaches the host from",
      { code: "unreachable", container_host: true } as const,
      { address: "http://localhost:11434/v1" },
      "Check finished: The check failed. The server could not be reached. Here localhost is the gateway's own container, not this computer. The gateway reaches this computer at host.docker.internal: http://host.docker.internal:11434/v1.",
    ],
    [
      "no endpoint for the protocol",
      { code: "endpoint_missing", status: 404 } as const,
      { protocol: "openai_chat" as const },
      "Check finished: The check failed. The server has no endpoint for this route's protocol at this address. This route speaks Chat Completions. Check the address (it often ends in /v1), or edit the route and try Responses or Messages under Advanced › Connection.",
    ],
    [
      "a local timeout",
      { code: "timeout" } as const,
      { local: true },
      "Check finished: The check failed. The server did not answer in time. A local model may be busy with another task or slow on this computer. Try again when it's idle; you can give local checks longer with [relay] local_check_timeout in gateway.toml.",
    ],
    // Each of nextStep()'s other ways, since a wrong key there would only be heard.
    [
      "a localhost route where host.docker.internal doesn't resolve",
      { code: "unreachable", container_host: false } as const,
      { address: "http://localhost:11434/v1" },
      "Check finished: The check failed. The server could not be reached. Here localhost is the gateway's own container, not this computer, and host.docker.internal doesn't resolve on this installation. Use this computer's network address instead, with the same port.",
    ],
    [
      "a localhost route on a gateway too old to say",
      { code: "unreachable" } as const,
      { address: "http://localhost:11434/v1" },
      "Check finished: The check failed. The server could not be reached. Here localhost is the gateway's own container, not this computer. With Docker Desktop (Mac or Windows), a model server on this computer is at host.docker.internal: http://host.docker.internal:11434/v1. On Linux, use the computer's network address instead.",
    ],
    [
      "a hosted timeout",
      { code: "timeout" } as const,
      { local: false },
      "Check finished: The check failed. The server did not answer in time. The server may be busy. Try again in a moment.",
    ],
    [
      "a context too small",
      { code: "context_too_small" } as const,
      { local: true },
      "Check finished: The check failed. The model server's context is too small for tasks. Give Ollama more room, such as 32,768 tokens, then check the route again:",
    ],
  ])("WEB-U-298: %s", async (_, failure, props, words) => {
    expect(await announce(failure, props)).toBe(words);
  });

  it("announces the verdict alone when there's no next step", async () => {
    expect(await announce({ code: "auth_rejected", status: 401 }, {})).toBe(
      "Check finished: The check failed. The server refused the route’s key.",
    );
  });

  it("shows no suggestion for a context with another failure", async () => {
    await announce(
      {
        code: "upstream_error",
        status: 400,
        suggestion: { field: "context_window", value: "4096", reason: "x" },
      },
      {},
    );
    const result = screen.getByRole("region", { name: "Check result" });
    expect(
      within(result).queryByText(
        "The provider refused this route's tool settings.",
      ),
    ).toBeNull();
    expect(
      within(result).queryByRole("button", { name: /^Set Context window/ }),
    ).toBeNull();
  });
});

// WEB-U-358 (a pass over the pages before 17.0): a route without its key says so in one sentence, with the way to add
// it as a real link on its own (not run on in the refusal's colour), and the button still reads Run check, since
// nothing ran.
describe("WEB-U-358 a check refused for a missing key", () => {
  it("says it once, links on its own, and offers Run check", async () => {
    const { dialog } = show({}, { key_missing: true });
    run(dialog);
    const refusal = await within(dialog).findByText(
      "This route has no key yet.",
    );
    expect(refusal).toHaveClass("field-error");
    expect(refusal).toHaveTextContent(/^This route has no key yet\.$/);
    const link = within(dialog).getByRole("link", {
      name: "Add it under Keys › Shared",
    });
    expect(link).toHaveAttribute("href", "/settings/keys/shared");
    expect(refusal.contains(link)).toBe(false);
    expect(
      within(dialog).getByRole("button", { name: "Run check" }),
    ).toBeVisible();
    expect(
      within(dialog).queryByRole("button", { name: "Run again" }),
    ).toBeNull();
  });
});
