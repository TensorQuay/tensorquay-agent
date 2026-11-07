import { templates } from "./fixtures.mjs";
import { withDesktop } from "./screen.mjs";

const rejected = (status, code) => ({
  status,
  body: {
    error: {
      code,
      message: "The fixture rejected this change.",
      retryable: false,
    },
  },
});

/** Mirrors keyed synchronous switching; only a subsequent run emits the changed event. */
export function switchEnvironment(conversation, input, key, metadata) {
  if (typeof key !== "string" || !/^[!-~]{1,128}$/.test(key))
    return rejected(400, "bad_request");
  const fingerprint = JSON.stringify({ environment: input });
  const previous = conversation.updates.get(key);
  if (previous)
    return previous.fingerprint === fingerprint
      ? { status: 200, body: previous.response }
      : rejected(422, "idempotency_key_reused");
  if (conversation.running) return rejected(409, "run_active");
  if (
    !withDesktop(templates).items.some(
      (template) => template.name === input.template,
    )
  )
    return rejected(422, "unknown_template");
  if (conversation.updates.size >= 1000) return rejected(429, "busy");
  if (conversation.template !== input.template) {
    conversation.template = input.template;
    conversation.updated_at = new Date().toISOString();
  }
  const response = metadata(conversation);
  conversation.updates.set(key, { fingerprint, response });
  return { status: 200, body: response };
}
