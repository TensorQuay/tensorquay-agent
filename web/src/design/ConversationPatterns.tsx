"use client";

import { ActivityGroup } from "../transcript/ActivityGroup";
import { Composer } from "../transcript/Composer";
import { present } from "../transcript/present";
import { apply, emptyTranscript } from "../transcript/reducer";

import { ConversationBarExample } from "./ConversationBarExample";
import { riverFrames } from "./river-fixture";

/** Exercises the conversation controls and work disclosures in each durable state. */
export function ConversationPatterns({ theme }: { theme: "light" | "dark" }) {
  const complete = present(riverFrames().reduce(apply, emptyTranscript()))[0];
  const running = present(
    riverFrames().slice(0, 14).reduce(apply, emptyTranscript()),
  )[0];
  return (
    <section
      className="design-panel stack"
      data-theme={theme}
      aria-label={`${theme} task patterns`}
    >
      <h2 className="type-heading">Task patterns · {theme}</h2>
      {(["idle", "sending", "running"] as const).map((state) => (
        <section className="stack" key={state}>
          <h3 className="type-small">{state}</h3>
          <ConversationBarExample state={state} />
          <Composer
            run={{
              id: state === "running" ? "example-run" : null,
              status: state === "running" ? "running" : "idle",
            }}
            sandbox="ready"
            sending={state === "sending"}
            cancelling={false}
            onSend={() => Promise.resolve(true)}
            onStop={() => Promise.resolve()}
          />
        </section>
      ))}
      {running && <ActivityGroup turn={running} />}
      {complete && (
        <>
          <ActivityGroup turn={complete} />
          <ActivityGroup
            turn={{
              ...complete,
              status: "failed",
              work: complete.work.map((item) =>
                item.kind === "tool"
                  ? {
                      ...item,
                      activity: {
                        ...item.activity,
                        tool: { ...item.activity.tool, status: "error" },
                      },
                    }
                  : item,
              ),
            }}
          />
        </>
      )}
    </section>
  );
}
