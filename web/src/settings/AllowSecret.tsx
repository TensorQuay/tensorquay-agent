"use client";

import { useState } from "react";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";

import { keyName } from "./secret-copy";

/**
 * A save refused because the secret it names may not be used here yet (vault slice 3: 409 secret_not_allowed). It
 * offers to allow it; the person then saves again themselves (WEB-U-054).
 */
export function AllowSecret({
  secret,
  consumer,
  copy,
  allow,
}: {
  secret: string;
  /** The route or connector the secret would be allowed for. */
  consumer: string;
  copy: {
    notAllowed: MessageKey;
    allowFor: MessageKey;
    allowed: MessageKey;
    failed: MessageKey;
  };
  allow: () => Promise<void>;
}) {
  const t = useT();
  const [state, setState] = useState<"idle" | "pending" | "done" | "failed">(
    "idle",
  );
  return (
    <div className="stack">
      <p className="field-error">
        {t(copy.notAllowed, { ref: keyName(secret) })}
      </p>
      {state === "done" ? (
        <p className="type-small" role="status">
          {t(copy.allowed)}
        </p>
      ) : (
        <div>
          <Button
            variant="secondary"
            type="button"
            loading={state === "pending"}
            onClick={() => {
              setState("pending");
              allow().then(
                () => setState("done"),
                () => setState("failed"),
              );
            }}
          >
            {t(copy.allowFor, { name: consumer })}
          </Button>
        </div>
      )}
      {state === "failed" && <p className="field-error">{t(copy.failed)}</p>}
    </div>
  );
}
