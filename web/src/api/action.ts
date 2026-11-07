import type { Accepted, MessageAccepted } from "../protocol/generated/api";
import { parseApi } from "../protocol/validate";

import { newActionKey } from "./action-key";
import { ApiError } from "./error";
import type { Transport } from "./transport";

type ExecutionStatus = "queued" | "delivered" | "failed" | null;
export type ActionResult = Accepted & { executionStatus: ExecutionStatus };
/** A message's acceptance: its id, and whether it waits for the running run (GW-I-129). */
export type MessageResult = MessageAccepted & {
  executionStatus: ExecutionStatus;
};

/** The operations a durable action may be. */
export type ActionOperation =
  | "message"
  | "cancel"
  | "createConversation"
  | "updateConversation"
  | "switchEnvironment"
  | "teamAnswer"
  | "teamStop"
  | "screenSignOut";

/** One user intent owns one immutable body and key, including automatic and explicit retries. */
export class Action<T = ActionResult> {
  private readonly key = newActionKey();

  constructor(
    private readonly transport: Transport,
    private readonly operation: ActionOperation,
    private readonly parameters: Record<string, string>,
    private readonly body: string,
    private readonly decode: (value: unknown, headers: Headers) => T,
  ) {}

  async execute(signal?: AbortSignal): Promise<T> {
    for (let attempt = 0; ; attempt++) {
      try {
        return await this.transport.request(this.operation, this.decode, {
          parameters: this.parameters,
          body: this.body,
          key: this.key,
          signal,
        });
      } catch (error) {
        if (
          !(error instanceof ApiError) ||
          error.status !== 429 ||
          attempt >= 2 ||
          (error.retryAfter ?? 1) > 10
        )
          throw error;
        await this.transport.environment.sleep(
          (error.retryAfter ?? 1) * 1000,
          signal,
        );
      }
    }
  }
}

function executionStatus(headers: Headers): ExecutionStatus {
  const status = headers.get("x-tqa-execution-status");
  return status === "queued" || status === "delivered" || status === "failed"
    ? status
    : null;
}

/** Decodes the durable acceptance and its current execution status. */
export function accepted(value: unknown, headers: Headers): ActionResult {
  return {
    ...parseApi(value, "Accepted"),
    executionStatus: executionStatus(headers),
  };
}

/** Decodes a message's acceptance, which also names the message and says whether it is held. */
export function messageAccepted(
  value: unknown,
  headers: Headers,
): MessageResult {
  return {
    ...parseApi(value, "MessageAccepted"),
    executionStatus: executionStatus(headers),
  };
}
