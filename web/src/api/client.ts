import type {
  AcceptInvitation,
  Bootstrap,
  ChangeEnvironment,
  CreateConversation,
  Login,
  Message,
  ForkWorkspace,
  NewWorkspace,
  PatchConversation,
  UpdateWorkspace,
} from "../protocol/generated/api";
import { parseApi } from "../protocol/validate";

import {
  accepted,
  Action,
  type ActionOperation,
  messageAccepted,
  type MessageResult,
} from "./action";
import { pathFor, type Endpoint, type Operation } from "./operations";
import {
  delay,
  Transport,
  type Environment,
  type RequestOptions,
} from "./transport";

export { ApiError } from "./error";

/** Browser requests stay on the gateway origin; the page server receives no credentials. */
export class ApiClient {
  private readonly transport: Transport;
  constructor(environment: Partial<Environment> = {}) {
    this.transport = new Transport({
      fetch: (...args) => globalThis.fetch(...args),
      redirect: (path) => {
        window.location.assign(path);
      },
      currentPath: () => window.location.pathname + window.location.search,
      sleep: delay,
      ...environment,
    });
  }
  bootstrap(body: Bootstrap, signal?: AbortSignal) {
    return this.transport.request(
      "bootstrap",
      (value) => parseApi(value, "User"),
      { body: JSON.stringify(body), signal },
    );
  }
  login(body: Login, signal?: AbortSignal) {
    return this.transport.request("login", (value) => parseApi(value, "User"), {
      body: JSON.stringify(body),
      signal,
    });
  }
  logout(signal?: AbortSignal) {
    return this.transport.request("logout", () => undefined, { signal });
  }
  me(signal?: AbortSignal) {
    return this.transport.request("me", (value) => parseApi(value, "User"), {
      signal,
    });
  }
  /**
   * A durable user action on this client's transport (one body, one Idempotency-Key across retries), for actions that
   * live with their pages (an agent team's answer and stop).
   */
  action<T>(
    operation: ActionOperation,
    parameters: Record<string, string>,
    body: string,
    decode: (value: unknown, headers: Headers) => T,
  ) {
    return new Action(this.transport, operation, parameters, body, decode);
  }
  /** One request on this client's transport, for requests that live with their pages (the admin client). */
  request<T>(
    operation: Operation | Endpoint,
    decode: (value: unknown, headers: Headers) => T,
    options?: RequestOptions,
  ) {
    return this.transport.request(operation, decode, options);
  }
  models(signal?: AbortSignal) {
    return this.transport.request(
      "models",
      (value) => parseApi(value, "Models"),
      { signal },
    );
  }
  /**
   * `all` asks for every user's tasks as metadata (administrators only; other users' titles are withheld); `q`
   * searches the caller's own titles as literal text, whatever `all` says (GW-I-121). `top` leaves helpers out, for
   * their lead carries them (ADR-0034 S6); the gateway ignores it with `q`, so a search still finds a helper.
   */
  conversations(
    cursor?: string,
    signal?: AbortSignal,
    all = false,
    q = "",
    top = false,
  ) {
    const query = new URLSearchParams(cursor ? { cursor } : {});
    if (all) query.set("all", "true");
    if (q) query.set("q", q);
    if (top) query.set("scope", "top");
    return this.transport.request(
      "conversations",
      (value) => parseApi(value, "ConversationPage"),
      { query, signal },
    );
  }
  templates(signal?: AbortSignal) {
    return this.transport.request(
      "templates",
      (value) => parseApi(value, "Templates"),
      { signal },
    );
  }
  /** A desktop task's stored screen notices, oldest first (ADR-0039 §6). */
  screenNotices(id: string, signal?: AbortSignal) {
    return this.transport.request(
      "screenNotices",
      (value) => parseApi(value, "ScreenNotices"),
      { parameters: { id }, signal },
    );
  }
  /**
   * Sign the desktop's browser out everywhere (ADR-0039 §6): accepted with no body (202); the outcome arrives as a screen
   * notice, signed out or not.
   */
  screenSignOut(id: string) {
    return new Action<void>(
      this.transport,
      "screenSignOut",
      { id },
      JSON.stringify({}),
      () => undefined,
    );
  }
  switchEnvironment(id: string, body: ChangeEnvironment) {
    return new Action(
      this.transport,
      "switchEnvironment",
      { id },
      JSON.stringify(body),
      (value) => parseApi(value, "Conversation"),
    );
  }
  createConversation(body: CreateConversation) {
    return new Action(
      this.transport,
      "createConversation",
      {},
      JSON.stringify(body),
      (value) => parseApi(value, "Conversation"),
    );
  }
  conversation(id: string, signal?: AbortSignal) {
    return this.transport.request(
      "conversation",
      (value) => parseApi(value, "Conversation"),
      { parameters: { id }, signal },
    );
  }
  updateConversation(id: string, body: PatchConversation) {
    return new Action(
      this.transport,
      "updateConversation",
      { id },
      JSON.stringify(body),
      (value) => parseApi(value, "Conversation"),
    );
  }
  /** `withHelpers` deletes a lead task with its helpers; without it, a lead with helpers is `409 has_helpers`. */
  deleteConversation(id: string, signal?: AbortSignal, withHelpers = false) {
    return this.transport.request("deleteConversation", () => undefined, {
      parameters: { id },
      ...(withHelpers
        ? { query: new URLSearchParams({ with_helpers: "true" }) }
        : {}),
      signal,
    });
  }
  message(id: string, body: Message): Action<MessageResult> {
    return new Action(
      this.transport,
      "message",
      { id },
      JSON.stringify(body),
      messageAccepted,
    );
  }
  /** The task's held and dropped messages, and those sent in the last day (GW-I-129), oldest first. */
  queuedMessages(id: string, signal?: AbortSignal) {
    return this.transport.request(
      "queuedMessages",
      (value) => parseApi(value, "QueuedMessages"),
      { parameters: { id }, signal },
    );
  }
  /** Cancels a held message, or dismisses a dropped one. */
  cancelQueuedMessage(id: string, message_id: string, signal?: AbortSignal) {
    return this.transport.request("cancelQueuedMessage", () => undefined, {
      parameters: { id, message_id },
      signal,
    });
  }
  cancel(id: string, run_id: string): Action {
    return new Action(this.transport, "cancel", { id, run_id }, "{}", accepted);
  }
  users(signal?: AbortSignal) {
    return this.transport.request(
      "users",
      (value) => parseApi(value, "Accounts"),
      { signal },
    );
  }
  acceptInvitation(body: AcceptInvitation, signal?: AbortSignal) {
    return this.transport.request(
      "acceptInvitation",
      (value) => parseApi(value, "User"),
      { body: JSON.stringify(body), signal },
    );
  }
  /** One page of the task's stored key uses, newest first (ADR-0037); `before` is the previous page's `next`. */
  keyUses(id: string, before: string | null, signal?: AbortSignal) {
    const query = new URLSearchParams({ limit: "100" });
    if (before) query.set("before", before);
    return this.transport.request(
      "keyUses",
      (value) => parseApi(value, "TaskKeyUses"),
      { parameters: { id }, query, signal },
    );
  }
  /** One folder of the task's workspace; `path` "" is the root. */
  files(id: string, path: string, after?: string, signal?: AbortSignal) {
    const query = new URLSearchParams({ path });
    if (after) query.set("after", after);
    return this.transport.request(
      "files",
      (value) => parseApi(value, "FilePage"),
      { parameters: { id }, query, signal },
    );
  }
  /** Your workspaces and those shared with you (task 076). */
  workspaces(signal?: AbortSignal) {
    return this.transport.request(
      "workspaces",
      (value) => parseApi(value, "Workspaces"),
      { signal },
    );
  }
  /**
   * One page of the Workspaces page's list (GW-I-117, GW-I-122): `sort` and `filter` as the gateway takes them, `q` a
   * search in the names the reader may read, `cursor` the previous page's `next_cursor`.
   */
  workspacePage(
    {
      limit,
      sort,
      filter,
      q,
      cursor,
    }: {
      limit: number;
      sort: "recent" | "name";
      filter: "all" | "in_use" | "idle" | "shared_with_me";
      q: string;
      cursor?: string;
    },
    signal?: AbortSignal,
  ) {
    const query = new URLSearchParams({ limit: String(limit), sort });
    if (filter !== "all") query.set("filter", filter);
    if (q) query.set("q", q);
    if (cursor) query.set("cursor", cursor);
    return this.transport.request(
      "workspaces",
      (value) => parseApi(value, "Workspaces"),
      { query, signal },
    );
  }
  workspace(id: string, signal?: AbortSignal) {
    return this.transport.request(
      "workspace",
      (value) => parseApi(value, "Workspace"),
      { parameters: { id }, signal },
    );
  }
  createWorkspace(body: NewWorkspace, signal?: AbortSignal) {
    return this.transport.request(
      "createWorkspace",
      (value) => parseApi(value, "Workspace"),
      { body: JSON.stringify(body), signal },
    );
  }
  updateWorkspace(id: string, body: UpdateWorkspace, signal?: AbortSignal) {
    return this.transport.request(
      "updateWorkspace",
      (value) => parseApi(value, "Workspace"),
      { parameters: { id }, body: JSON.stringify(body), signal },
    );
  }
  /** `withTasks` also deletes the owner's tasks on it (refused while anyone else has tasks there). */
  deleteWorkspace(id: string, withTasks: boolean, signal?: AbortSignal) {
    return this.transport.request("deleteWorkspace", () => undefined, {
      parameters: { id },
      ...(withTasks
        ? { query: new URLSearchParams({ with_conversations: "true" }) }
        : {}),
      signal,
    });
  }
  /** Starts a copy in the background; the new workspace is `forking` until it is ready. */
  forkWorkspace(id: string, body: ForkWorkspace, signal?: AbortSignal) {
    return this.transport.request(
      "forkWorkspace",
      (value) => parseApi(value, "Workspace"),
      { parameters: { id }, body: JSON.stringify(body), signal },
    );
  }
  /** The files one run added, modified or deleted. */
  runChanges(id: string, run_id: string, signal?: AbortSignal) {
    return this.transport.request(
      "runChanges",
      (value) => parseApi(value, "RunChanges"),
      { parameters: { id, run_id }, signal },
    );
  }
  /** A file's bytes, for a preview the browser draws itself (an image, finding F1); downloads use downloadUrl. */
  fileBlob(id: string, path: string, signal?: AbortSignal) {
    return this.transport.request(
      "fileDownload",
      (value) => {
        // Checked by shape: a Blob from another realm (the fetch implementation's) is still the file's bytes.
        if (
          typeof value !== "object" ||
          value === null ||
          typeof (value as Blob).arrayBuffer !== "function"
        )
          // copy-lint: not user copy (internal diagnostic)
          throw new Error("Expected the file's bytes");
        return value as Blob;
      },
      {
        parameters: { id },
        query: new URLSearchParams({ path }),
        read: "blob",
        signal,
      },
    );
  }
  /**
   * A desktop step's screenshot, as bytes (ADR-0039 §9): screenshots are served no-store, so the replay keeps the one it
   * fetches ahead in memory itself (WEB-U-268).
   */
  screenshotBlob(id: string, screenshot: string, signal?: AbortSignal) {
    return this.transport.request(
      "screenshot",
      (value) => {
        if (
          typeof value !== "object" ||
          value === null ||
          typeof (value as Blob).arrayBuffer !== "function"
        )
          // copy-lint: not user copy (internal diagnostic)
          throw new Error("Expected the screenshot's bytes");
        return value as Blob;
      },
      { parameters: { id, screenshot }, read: "blob", signal },
    );
  }
  /** The browser downloads through a plain link, so the session cookie applies and nothing is buffered here. */
  downloadUrl(id: string, path: string): string {
    return pathFor("fileDownload", { id }, new URLSearchParams({ path }));
  }
  /** An HTML page for the panel's frame; the gateway sandboxes it by its headers (the file previews design). */
  viewUrl(id: string, path: string): string {
    return pathFor("fileView", { id }, new URLSearchParams({ path }));
  }
}

export const api = new ApiClient();
