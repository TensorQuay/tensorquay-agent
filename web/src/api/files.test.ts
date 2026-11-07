// @vitest-environment node
import { describe, expect, it, vi } from "vitest";

import { conversationId } from "../test/frames";

import { ApiClient, ApiError } from "./client";
import { fileContent, runChangeDiff } from "./previews";
import { uploadFile, type UploadRequest } from "./upload";

function setup() {
  const fetch = vi.fn<typeof globalThis.fetch>();
  const client = new ApiClient({
    fetch,
    redirect: vi.fn(),
    sleep: vi.fn().mockResolvedValue(undefined),
    currentPath: () => `/c/${conversationId}`,
  });
  return { client, fetch };
}
const base = `/api/v1/conversations/${conversationId}`;
const modified = "2026-01-01T00:00:00Z";

describe("WEB-U-022 workspace file requests", () => {
  it("lists one folder with its cursor and reads one file", async () => {
    const { client, fetch } = setup();
    const page = {
      path: "src",
      items: [
        { name: "app.py", kind: "file", size: 12, modified_at: modified },
      ],
      next_after: "app.py",
    };
    fetch.mockResolvedValueOnce(Response.json(page));
    await expect(client.files(conversationId, "src", "a b")).resolves.toEqual(
      page,
    );
    expect(fetch.mock.calls[0]?.[0]).toBe(`${base}/files?path=src&after=a+b`);

    fetch.mockResolvedValueOnce(Response.json({ ...page, next_after: null }));
    await client.files(conversationId, "");
    expect(fetch.mock.calls[1]?.[0]).toBe(`${base}/files?path=`);

    const content = {
      path: "src/app.py",
      size: 12,
      modified_at: modified,
      etag: '"12-1-2"',
      binary: false,
      truncated: false,
      text: "print('hi')\n",
    };
    fetch.mockResolvedValueOnce(Response.json(content));
    await expect(
      fileContent(client, conversationId, "src/app.py"),
    ).resolves.toEqual(content);
    expect(fetch.mock.calls[2]?.[0]).toBe(
      `${base}/files/content?path=src%2Fapp.py`,
    );
  });

  it("links downloads to the gateway and keeps error codes", async () => {
    const { client, fetch } = setup();
    expect(client.downloadUrl(conversationId, "data/a b.csv")).toBe(
      `${base}/files/download?path=data%2Fa+b.csv`,
    );
    fetch.mockResolvedValueOnce(
      Response.json(
        {
          error: {
            code: "files_unavailable",
            message: "Files are not available.",
            retryable: false,
          },
        },
        { status: 409 },
      ),
    );
    await expect(client.files(conversationId, "")).rejects.toMatchObject({
      status: 409,
      code: "files_unavailable",
    });
  });
});

const progressEvent = (loaded: number, total: number) =>
  ({ loaded, total, lengthComputable: total > 0 }) as ProgressEvent;

describe("WEB-U-024 run changes requests", () => {
  it("reads a run's changed files and one file's diff", async () => {
    const { client, fetch } = setup();
    const changes = {
      run_id: "run-1",
      live: true,
      running: false,
      truncated: false,
      files: [
        {
          path: "report.md",
          status: "added",
          added: 3,
          removed: 0,
          binary: false,
          too_large: false,
        },
      ],
    };
    fetch.mockResolvedValueOnce(Response.json(changes));
    await expect(client.runChanges(conversationId, "run-1")).resolves.toEqual(
      changes,
    );
    expect(fetch.mock.calls[0]?.[0]).toBe(`${base}/runs/run-1/changes`);
    const diff = {
      path: "docs/a b.md",
      status: "modified",
      unified: "--- a/docs/a b.md\n+++ b/docs/a b.md\n",
      binary: false,
      too_large: false,
      truncated: false,
    };
    fetch.mockResolvedValueOnce(Response.json(diff));
    await expect(
      runChangeDiff(client, conversationId, "run-1", "docs/a b.md"),
    ).resolves.toEqual(diff);
    expect(fetch.mock.calls[1]?.[0]).toBe(
      `${base}/runs/run-1/changes/diff?path=docs%2Fa+b.md`,
    );
  });
});

/** A scripted XMLHttpRequest: the test drives progress and the answer. */
class FakeRequest implements UploadRequest {
  method = "";
  url = "";
  headers: Record<string, string> = {};
  body: Blob | null = null;
  status = 0;
  responseText = "";
  private responseHeaders: Record<string, string> = {};
  upload: UploadRequest["upload"] = { onprogress: null };
  onload: UploadRequest["onload"] = null;
  onerror: UploadRequest["onerror"] = null;
  onabort: UploadRequest["onabort"] = null;
  aborted = false;
  open(method: string, url: string) {
    this.method = method;
    this.url = url;
  }
  setRequestHeader(name: string, value: string) {
    this.headers[name] = value;
  }
  getResponseHeader(name: string) {
    return this.responseHeaders[name.toLowerCase()] ?? null;
  }
  send(body: Blob) {
    this.body = body;
  }
  abort() {
    this.aborted = true;
    this.onabort?.(progressEvent(0, 0));
  }
  answer(status: number, value: unknown, headers: Record<string, string> = {}) {
    this.status = status;
    this.responseText = JSON.stringify(value);
    this.responseHeaders = { "content-type": "application/json", ...headers };
    this.onload?.(progressEvent(0, 0));
  }
}

describe("WEB-U-020 uploads", () => {
  const file = new File(["a,b\n1,2\n"], "Q3 report.csv");

  it("streams the raw file with its name and reports progress", async () => {
    const request = new FakeRequest();
    const progress = vi.fn();
    const pending = uploadFile(conversationId, file, {
      onProgress: progress,
      request: () => request,
    });
    expect(request.method).toBe("POST");
    expect(request.url).toBe(`${base}/uploads?name=Q3+report.csv`);
    expect(request.headers["content-type"]).toBe("application/octet-stream");
    expect(request.body).toBe(file);
    request.upload.onprogress?.(progressEvent(4, 8));
    expect(progress).toHaveBeenCalledWith(4, 8);
    const stored = {
      path: "uploads/Q3_report.csv",
      name: "Q3_report.csv",
      size: 8,
    };
    request.answer(201, stored);
    await expect(pending).resolves.toEqual(stored);
  });

  it("turns a refusal into the gateway's error code", async () => {
    const request = new FakeRequest();
    const pending = uploadFile(conversationId, file, {
      request: () => request,
    });
    request.answer(409, {
      error: {
        code: "uploads_full",
        message: "The upload space is full.",
        retryable: false,
      },
    });
    await expect(pending).rejects.toMatchObject({
      status: 409,
      code: "uploads_full",
    });
  });

  it("rejects an unreadable answer, a network failure and a cancellation", async () => {
    const invalid = new FakeRequest();
    const first = uploadFile(conversationId, file, { request: () => invalid });
    invalid.answer(201, { path: "../escape" });
    await expect(first).rejects.toMatchObject({ code: "invalid_response" });

    const offline = new FakeRequest();
    const second = uploadFile(conversationId, file, { request: () => offline });
    offline.onerror?.(progressEvent(0, 0));
    await expect(second).rejects.toMatchObject({ code: "network" });

    const cancelled = new FakeRequest();
    const controller = new AbortController();
    const third = uploadFile(conversationId, file, {
      request: () => cancelled,
      signal: controller.signal,
    });
    controller.abort();
    expect(cancelled.aborted).toBe(true);
    await expect(third).rejects.toBeInstanceOf(ApiError);
    await expect(third).rejects.toMatchObject({ code: "cancelled" });
  });
});
