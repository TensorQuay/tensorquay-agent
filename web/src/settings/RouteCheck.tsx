"use client";

import Link from "next/link";
import { useEffect, useId, useState } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageValues } from "../i18n/message";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";

import type { ProbeResult, RoutesApi } from "./admin/routes";
import { failureKey, probeRefusal, STEPS, WARNINGS } from "./probe-copy";
import { containerHostAddress } from "./route-local";
import { PROTOCOL_NAMES } from "./RouteBasics";
import type { Preset } from "./RouteForm";

/**
 * The Max output a model that thinks before answering needs (the F16 check): 4096 is what candidate 12's qwen3:4b spent
 * thinking on its first step (CORE-U-072), and thinking runs past it at 16384 still.
 */
const THINKING_ROOM = 16384;

type Protocol = keyof typeof PROTOCOL_NAMES;

type State =
  | { kind: "ready" }
  | { kind: "running" }
  | { kind: "done"; result: ProbeResult }
  | { kind: "refused"; copy: [MessageKey, MessageValues]; keyMissing: boolean };

/**
 * Checks one stored route the way a task will use it (099b-2): nothing runs, or spends tokens, until the
 * administrator asks. A failed check is a result to read, and a refusal says what to do.
 */
export function RouteCheck({
  name,
  label = name,
  local = false,
  maxOutput = null,
  contextWindow = null,
  address = "",
  protocol,
  api,
  onClose,
  onChecked,
  onSuggest,
}: {
  name: string;
  /** What the page calls the route: its display name (the check itself runs by `name`). */
  label?: string;
  /** The route reaches a local model, whose check the gateway gives longer (WEB-U-095). */
  local?: boolean;
  /** The route's Max output, null when it sets none: a model that thinks unasked is offered room (WEB-U-133). */
  maxOutput?: number | null;
  /** The route's Context window, null when it sets none: a local server's own size is offered when it differs (WEB-U-269). */
  contextWindow?: number | null;
  /** The stored route's address: one at localhost can't reach this computer from the gateway's container. */
  address?: string;
  /** The stored route's protocol: a missing endpoint names it and the others to try. */
  protocol?: Protocol | undefined;
  api: RoutesApi;
  onClose: () => void;
  /** A check ran and the gateway stored its result, so the route's listed state changed. */
  onChecked?: () => void;
  /** Opens the route's form with a suggested setting applied, unsaved (WEB-U-058). */
  onSuggest: (preset: Preset) => void;
}) {
  const t = useT();
  const [state, setState] = useState<State>({ kind: "ready" });
  async function run() {
    setState({ kind: "running" });
    try {
      setState({ kind: "done", result: await api.probe(name, newActionKey()) });
      onChecked?.();
    } catch (error) {
      setState({
        kind: "refused",
        copy: probeRefusal(error),
        keyMissing: error instanceof ApiError && error.code === "key_missing",
      });
    }
  }
  return (
    <Dialog
      open
      title={t("models.checkTitle", { name: label })}
      onClose={onClose}
    >
      <div className="stack">
        <p className="type-small muted">
          {t(local ? "models.checkIntroLocal" : "models.checkIntro")}
        </p>
        {/* One status line, there from the start: it says the check is running, then how it ended, so the outcome is
            announced as well as the start (WEB-U-271). */}
        <p className="sr-only" role="status">
          {state.kind === "running"
            ? t(local ? "models.checkingLocal" : "models.checking")
            : state.kind === "done"
              ? t("models.checkAnnounceDone", {
                  // The next step, when the result has one, is heard with the verdict (the c15 accessibility review).
                  verdict: [
                    verdict(state.result, t),
                    nextStep(state.result, { local, address, protocol }, t),
                  ]
                    .filter(Boolean)
                    .join(" "),
                })
              : state.kind === "refused"
                ? t("models.checkAnnounceRefused", {
                    reason: t(...state.copy),
                  })
                : ""}
        </p>
        {state.kind === "running" && <CheckingLine local={local} />}
        {state.kind === "done" && (
          <CheckResult
            result={state.result}
            local={local}
            maxOutput={maxOutput}
            contextWindow={contextWindow}
            address={address}
            protocol={protocol}
            onSuggest={onSuggest}
          />
        )}
        {/* The refusal in one sentence, and the way forward as a link of its own (WEB-U-358). */}
        {state.kind === "refused" && (
          <p className="field-error">{t(...state.copy)}</p>
        )}
        {state.kind === "refused" && state.keyMissing && (
          <p className="type-small">
            <Link href="/settings/keys/shared">{t("models.addKey")}</Link>
          </p>
        )}
        <div className="dialog-actions">
          <Button variant="secondary" type="button" onClick={onClose}>
            {t("common.done")}
          </Button>
          <Button loading={state.kind === "running"} onClick={() => void run()}>
            {t(
              // A refused check never ran, so it is still Run check (WEB-U-358).
              state.kind === "done" ? "models.runAgain" : "models.runCheck",
            )}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

/** The verdict, what was observed, and the server's own answer as inert text. */
function CheckResult({
  result,
  local,
  maxOutput,
  contextWindow,
  address,
  protocol,
  onSuggest,
}: {
  result: ProbeResult;
  /** A local model's timeout has its own next step: it may be busy or slow on this machine (WEB-U-264). */
  local: boolean;
  maxOutput: number | null;
  contextWindow: number | null;
  address: string;
  protocol: Protocol | undefined;
  onSuggest: (preset: Preset) => void;
}) {
  const t = useT();
  const id = useId();
  const { observed } = result;
  const yes = (value: boolean | null) =>
    t(
      value === null
        ? "models.notApplicable"
        : value
          ? "models.yes"
          : "models.no",
    );
  const rows: [MessageKey, string][] = [
    ["models.obsStreaming", yes(observed.streaming)],
    ["models.obsToolCall", t(STEPS[observed.tool_call])],
    ["models.obsRoundTrip", t(STEPS[observed.round_trip])],
    // GW-U-064: a two-line value through strict tools; a gateway without the step leaves the row out (WEB-U-061).
    ...(observed.strict_line_breaks === undefined
      ? []
      : ([
          ["models.obsStrictLineBreaks", t(STEPS[observed.strict_line_breaks])],
        ] as [MessageKey, string][])),
    [
      "models.obsUsage",
      observed.usage.reported
        ? t("models.usageDetail", {
            cached: yes(observed.usage.cached),
            reasoning: yes(observed.usage.reasoning),
          })
        : yes(false),
    ],
    ["models.obsReasoning", yes(observed.reasoning_accepted)],
    [
      "models.obsFirstByte",
      observed.first_byte_ms === null
        ? yes(null)
        : t("models.milliseconds", { n: observed.first_byte_ms }),
    ],
    ["models.obsTotal", t("models.milliseconds", { n: observed.total_ms })],
    // The model's speed, from a gateway that measures it (a first-hour check before 17.0).
    ...(observed.output_tokens_per_second === undefined
      ? []
      : ([
          [
            "models.obsSpeed",
            t("models.tokensPerSecond", {
              n: observed.output_tokens_per_second,
            }),
          ],
        ] as [MessageKey, string][])),
    [
      "models.obsTokens",
      t("models.tokensSpent", {
        prompt: observed.tokens.prompt,
        completion: observed.tokens.completion,
      }),
    ],
  ];
  const failure = observed.failure;
  // A model that thinks unasked needs room only when a Max output below it is set: blank or 16384 and up, there is
  // nothing to do, so neither the advice nor the action shows (WEB-U-133; a pass over the pages before 17.0).
  const needsRoom =
    observed.warnings.includes("thinks_unasked") &&
    maxOutput !== null &&
    maxOutput < THINKING_ROOM;
  // What the one click offers: the room, or half a smaller Context window, since Max output must fit the window and the
  // input needs the rest; offered only when above the limit set.
  const room =
    contextWindow === null
      ? THINKING_ROOM
      : Math.min(THINKING_ROOM, Math.floor(contextWindow / 2));
  const offersRoom = needsRoom && room > maxOutput;
  return (
    <section aria-labelledby={id} className="stack">
      <h2 id={id} className="sr-only">
        {t("models.checkResult")}
      </h2>
      <p className="type-body-strong">{verdict(result, t)}</p>
      {/* A local model's context too small for tasks: how to give it more, on each platform (WEB-U-269). */}
      {failure?.code === "context_too_small" && <ContextFix />}
      {/* A loopback address, unreachable: from the gateway's container it is the container itself (WEB-U-281). */}
      {failure?.code === "unreachable" && (
        <ContainerHost
          address={address}
          reaches={failure.container_host}
          onSuggest={onSuggest}
        />
      )}
      {/* No endpoint for the route's protocol: the address, or another protocol (WEB-U-287). */}
      {failure?.code === "endpoint_missing" && protocol && (
        <EndpointMissing protocol={protocol} />
      )}
      {/* A timeout says what to do next (a first-hour check before 17.0; WEB-U-264). */}
      {failure?.code === "timeout" && (
        <p className="type-small">
          {t(local ? "models.timeoutNextLocal" : "models.timeoutNext")}
        </p>
      )}
      {/* The verdict speaks first; what was observed is for whoever wants it (F7). */}
      <details className="check-details">
        <summary className="type-small">{t("models.checkDetails")}</summary>
        <dl className="check-list type-small">
          {rows.map(([term, detail]) => (
            <div key={term} className="check-row-item">
              <dt className="muted">{t(term)}</dt>
              <dd>{detail}</dd>
            </div>
          ))}
        </dl>
      </details>
      {observed.warnings
        .filter((warning) => warning !== "thinks_unasked" || needsRoom)
        .filter((warning) => warning !== "context_window_differs")
        .filter((warning) => warning !== "slow_answers")
        .map((warning) => (
          <p key={warning} className="type-small" role="note">
            {t(WARNINGS[warning])}
          </p>
        ))}
      {/* A model that thinks unasked needs room, which one click fills in when a lower limit is set (WEB-U-133). */}
      {offersRoom && (
        <div>
          <Button
            variant="secondary"
            onClick={() =>
              onSuggest({
                field: "max_output",
                value: String(room),
              })
            }
          >
            {t("models.suggestMaxOutput", { n: room })}
          </Button>
        </div>
      )}
      {/* The route's Context window apart from what the local server gives the model: one click sets it (WEB-U-269). */}
      {observed.warnings.includes("context_window_differs") &&
        observed.server_context && (
          <ContextDiffers
            size={observed.server_context.context_length}
            route={contextWindow}
            onSuggest={onSuggest}
          />
        )}
      {/* Below the minimum, a Context window matching the server's size wouldn't make tasks work, so it isn't offered
          (P2 #10): the fix is the server's. */}
      {failure?.suggestion && failure.code !== "context_too_small" && (
        <Suggestion suggestion={failure.suggestion} onSuggest={onSuggest} />
      )}
      {failure && (failure.status !== null || failure.excerpt) && (
        <div className="stack">
          <p className="type-label muted">{t("models.serverAnswer")}</p>
          {failure.status !== null && (
            <p className="type-small">
              {t("models.httpStatus", { status: failure.status })}
            </p>
          )}
          {failure.excerpt && (
            <pre className="type-code check-excerpt">{failure.excerpt}</pre>
          )}
        </div>
      )}
    </section>
  );
}

type Suggested = NonNullable<
  NonNullable<ProbeResult["observed"]["failure"]>["suggestion"]
>;
/** The settings this web can name and apply; the gateway's `reason` is fixed English, so our copy stands in for it. */
const SUGGESTIONS: Readonly<Record<string, MessageKey>> = {
  "schema_keywords=basic": "models.suggestKeywords",
  "strict=off": "models.suggestStrict",
  "stream_usage=false": "models.suggestStreamUsageOff",
};

/** What a failed check's 400 points at, as a hint only: the route changes when the person saves it (WEB-U-058). */
function Suggestion({
  suggestion,
  onSuggest,
}: {
  suggestion: Suggested;
  onSuggest: (preset: Preset) => void;
}) {
  const t = useT();
  // A context the server gives the model is offered by the differs note, never here: the gateway suggests it only with
  // context_too_small, where matching the route wouldn't make tasks work (WEB-U-290).
  if (suggestion.field === "context_window") return null;
  const copy = SUGGESTIONS[`${suggestion.field}=${suggestion.value}`];
  if (!copy) return <p className="type-small">{t("models.suggestOther")}</p>;
  return (
    <div className="stack">
      <p className="type-small">{t(copy)}</p>
      <div>
        <Button
          variant="secondary"
          onClick={() =>
            onSuggest({ field: suggestion.field, value: suggestion.value })
          }
        >
          {t("models.suggestEdit")}
        </Button>
      </div>
    </div>
  );
}

/** How often a running check's line counts the minutes. */
const ELAPSED_TICK_MS = 15_000;

/**
 * The running check's visible line. A local model may answer for minutes while the check waits on its progress (a
 * first-hour check before 17.0), so after the first minute the line says how long it has run, and that it is still
 * waiting. The status line above announces the check once; this one changes quietly.
 */
function CheckingLine({ local }: { local: boolean }) {
  const t = useT();
  const [started] = useState(() => Date.now());
  const [now, setNow] = useState(started);
  useEffect(() => {
    if (!local) return;
    const timer = setInterval(() => setNow(Date.now()), ELAPSED_TICK_MS);
    return () => clearInterval(timer);
  }, [local]);
  const minutes = Math.floor((now - started) / 60_000);
  return (
    <p className="type-small" aria-hidden="true">
      {!local
        ? t("models.checking")
        : minutes < 1
          ? t("models.checkingLocal")
          : t("models.checkingLocalLong", { minutes })}
    </p>
  );
}

/**
 * The check's verdict in one line: verified (slowly, with the model's speed, when a local model answers slowly: a
 * first-hour check before 17.0), or failed and why.
 */
function verdict(result: ProbeResult, t: ReturnType<typeof useT>): string {
  const {
    failure,
    warnings,
    output_tokens_per_second: speed,
  } = result.observed;
  if (result.state === "verified")
    return warnings.includes("slow_answers") && speed !== undefined
      ? t("models.checkVerifiedSlowly", { speed })
      : t("models.checkVerified");
  return `${t("models.checkFailed")}${failure ? ` ${failureWords(failure.code, result.observed.server_context, t)}` : ""}`;
}

/**
 * The one sentence that says what to do next, for the failures whose result shows one: a localhost route (WEB-U-281,
 * -287), no endpoint for the protocol (WEB-U-287), a timeout (WEB-U-264) or a context too small (WEB-U-269). The same
 * words the result shows; null for the rest.
 */
function nextStep(
  result: ProbeResult,
  route: { local: boolean; address: string; protocol: Protocol | undefined },
  t: ReturnType<typeof useT>,
): string | null {
  const failure = result.observed.failure;
  switch (failure?.code) {
    case "unreachable":
      return loopbackWords(route.address, failure.container_host, t);
    case "endpoint_missing":
      return route.protocol ? endpointWords(route.protocol, t) : null;
    case "timeout":
      return t(route.local ? "models.timeoutNextLocal" : "models.timeoutNext");
    case "context_too_small":
      return t("models.contextFixIntro");
    default:
      return null;
  }
}

/** A failure's words; a context too small names both sizes when the check read them (WEB-U-269). */
function failureWords(
  code: string,
  server: ProbeResult["observed"]["server_context"],
  t: ReturnType<typeof useT>,
): string {
  if (code === "context_too_small" && server)
    return t("models.failContextTooSmallSized", {
      context: server.context_length,
      minimum: server.minimum,
    });
  return t(failureKey(code));
}

/** How to give a local Ollama more context, on each platform, and how to see what it gives now. */
function ContextFix() {
  const t = useT();
  return (
    <div className="stack">
      <p className="type-small">{t("models.contextFixIntro")}</p>
      <ul className="type-small">
        <li>{t("models.contextFixMac")}</li>
        <li>{t("models.contextFixWindows")}</li>
        <li>{t("models.contextFixLinux")}</li>
        <li>{t("models.contextFixDocker")}</li>
      </ul>
      <p className="type-small">{t("models.contextFixCheck")}</p>
    </div>
  );
}

/** The route's Context window isn't the size the local server gives the model: say so, and set it in one click. */
function ContextDiffers({
  size,
  route,
  onSuggest,
}: {
  size: number;
  route: number | null;
  onSuggest: (preset: Preset) => void;
}) {
  const t = useT();
  return (
    <div className="stack">
      <p className="type-small" role="note">
        {route === null
          ? t("models.warnContextWindowUnset", { n: size })
          : t("models.warnContextWindowDiffers", { n: size, route })}
      </p>
      <div>
        <Button
          variant="secondary"
          onClick={() =>
            onSuggest({ field: "context_window", value: String(size) })
          }
        >
          {t("models.suggestContextWindow", { n: size })}
        </Button>
      </div>
    </div>
  );
}

/**
 * A route at localhost (or 127.x, ::1) the check could not reach: inside the gateway's container that is the container
 * itself, so a model server on this computer is at the container host's name. Says so, and changes it in one click.
 */
function ContainerHost({
  address,
  reaches,
  onSuggest,
}: {
  address: string;
  /** Whether host.docker.internal resolves from the gateway's container (GW-U-154); absent from an older gateway. */
  reaches: boolean | null | undefined;
  onSuggest: (preset: Preset) => void;
}) {
  const t = useT();
  const suggested = containerHostAddress(address);
  const words = loopbackWords(address, reaches, t);
  if (!suggested || !words) return null;
  // The one click only where the gateway can reach that name (WEB-U-287); without a word from the gateway, the words
  // say where it works and where it doesn't.
  if (reaches !== true) return <p className="type-small">{words}</p>;
  return (
    <div className="stack">
      <p className="type-small">{words}</p>
      <div>
        <Button
          variant="secondary"
          onClick={() => onSuggest({ field: "base_url", value: suggested })}
        >
          {t("models.suggestContainerHost")}
        </Button>
      </div>
    </div>
  );
}

/** A server with no endpoint for the route's protocol at its address: which protocol it speaks, and the others to try. */
function EndpointMissing({ protocol }: { protocol: Protocol }) {
  const t = useT();
  return <p className="type-small">{endpointWords(protocol, t)}</p>;
}

/** The protocol a route speaks and the two others to try (WEB-U-287), as the result and its announcement say it. */
function endpointWords(protocol: Protocol, t: ReturnType<typeof useT>) {
  const [first, second] = (Object.keys(PROTOCOL_NAMES) as Protocol[])
    .filter((each) => each !== protocol)
    .map((each) => t(PROTOCOL_NAMES[each]));
  return t("models.endpointMissingNext", {
    protocol: t(PROTOCOL_NAMES[protocol]),
    others: t("models.protocolOr", {
      first: first ?? "",
      second: second ?? "",
    }),
  });
}

/**
 * What a route at a loopback address needs instead (WEB-U-281, -287), as the result and its announcement say it: by
 * whether the gateway reaches host.docker.internal; null for any other address.
 */
function loopbackWords(
  address: string,
  reaches: boolean | null | undefined,
  t: ReturnType<typeof useT>,
): string | null {
  const suggested = containerHostAddress(address);
  if (!suggested) return null;
  const host = new URL(address).hostname;
  return t(
    reaches === true
      ? "models.unreachableLoopbackHost"
      : reaches === false
        ? "models.unreachableLoopbackNoHost"
        : "models.unreachableLoopback",
    { host, address: suggested },
  );
}
