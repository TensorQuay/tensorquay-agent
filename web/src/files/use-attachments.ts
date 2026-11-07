"use client";

import { useEffect, useRef, useState } from "react";

import { ApiError } from "../api/error";
import { uploadFile } from "../api/upload";

/**
 * The gateway's default per-file limit (answers-068 Q4), used until the signed-in account's own arrives (GW-I-142);
 * the gateway still answers upload_too_large for anything larger.
 */
export const MAX_FILE_BYTES = 25 * 1024 * 1024;
export const MAX_ATTACHMENTS = 32;

export type Attachment = {
  key: string;
  name: string;
  size: number;
  /** "held": chosen on New task, waiting for the task to exist (uploadHeld). */
  status: "held" | "uploading" | "done" | "failed";
  loaded: number;
  path: string | null;
  /** A gateway error code, or `too_large` when the file was refused before uploading. */
  error: string | null;
};

export type Attachments = ReturnType<typeof useAttachments>;

/**
 * Uploads chosen files into the task's workspace one at a time, in the order they were added: the gateway numbers a
 * repeated name as uploads arrive, so this keeps "name-2" on the file attached second. A message names only finished
 * uploads.
 */
export function useAttachments(
  /** The task to upload into; null on New task, where files are held until uploadHeld names the new task. */
  conversationId: string | null,
  upload = uploadFile,
  /** The installation's per-file limit, from the signed-in account (`limits.upload_bytes`). */
  limit = MAX_FILE_BYTES,
) {
  const [items, setItems] = useState<Attachment[]>([]);
  const [refused, setRefused] = useState(0);
  const count = useRef(0);
  const serial = useRef(0);
  const queue = useRef<{ key: string; file: File }[]>([]);
  const held = useRef(new Map<string, File>());
  const running = useRef<{ key: string; controller: AbortController } | null>(
    null,
  );
  useEffect(
    () => () => {
      queue.current = [];
      running.current?.controller.abort();
    },
    [],
  );
  const change = (key: string, patch: Partial<Attachment>) =>
    setItems((previous) =>
      previous.map((item) => (item.key === key ? { ...item, ...patch } : item)),
    );
  function next() {
    const waiting = running.current ? undefined : queue.current.shift();
    if (waiting) start(waiting.key, waiting.file);
  }
  function start(key: string, file: File) {
    const controller = new AbortController();
    running.current = { key, controller };
    if (conversationId === null) return;
    upload(conversationId, file, {
      signal: controller.signal,
      onProgress: (loaded) => change(key, { loaded }),
    })
      .then((stored) =>
        change(key, {
          status: "done",
          name: stored.name,
          size: stored.size,
          loaded: stored.size,
          path: stored.path,
        }),
      )
      .catch((error: unknown) => {
        if (controller.signal.aborted) return;
        change(key, {
          status: "failed",
          error: error instanceof ApiError ? error.code : "network",
        });
      })
      .finally(() => {
        if (running.current?.key !== key) return;
        running.current = null;
        next();
      });
  }
  function add(files: readonly File[]) {
    const room = Math.max(0, MAX_ATTACHMENTS - count.current);
    const taken = files.slice(0, room);
    setRefused(files.length - taken.length);
    count.current += taken.length;
    const added = taken.map((file): Attachment => {
      const tooLarge = file.size > limit;
      return {
        key: `attachment-${++serial.current}`,
        name: file.name,
        size: file.size,
        status: tooLarge
          ? "failed"
          : conversationId === null
            ? "held"
            : "uploading",
        loaded: 0,
        path: null,
        error: tooLarge ? "too_large" : null,
      };
    });
    setItems((previous) => [...previous, ...added]);
    taken.forEach((file, index) => {
      const item = added[index];
      if (item?.status === "uploading")
        queue.current.push({ key: item.key, file });
      if (item?.status === "held") held.current.set(item.key, file);
    });
    next();
  }
  function remove(key: string) {
    held.current.delete(key);
    queue.current = queue.current.filter((waiting) => waiting.key !== key);
    if (running.current?.key === key) {
      running.current.controller.abort();
      running.current = null;
      next();
    }
    count.current = Math.max(0, count.current - 1);
    setRefused(0);
    setItems((previous) => previous.filter((item) => item.key !== key));
  }
  function clear() {
    held.current.clear();
    queue.current = [];
    count.current = 0;
    setRefused(0);
    setItems([]);
  }
  /**
   * Uploads the held files, in the order they were added, to the task that now exists; answers every uploaded path,
   * or null when one failed (it shows as failed, and the others stay uploaded for a retry).
   */
  async function uploadHeld(id: string): Promise<string[] | null> {
    const paths: string[] = [];
    let failed = false;
    for (const item of items) {
      if (item.status === "done" && item.path) {
        paths.push(item.path);
        continue;
      }
      const file = held.current.get(item.key);
      if (!file) continue;
      change(item.key, { status: "uploading", loaded: 0, error: null });
      try {
        const stored = await upload(id, file, {
          onProgress: (loaded) => change(item.key, { loaded }),
        });
        held.current.delete(item.key);
        change(item.key, {
          status: "done",
          name: stored.name,
          size: stored.size,
          loaded: stored.size,
          path: stored.path,
        });
        paths.push(stored.path);
      } catch (error) {
        failed = true;
        change(item.key, {
          status: "failed",
          error: error instanceof ApiError ? error.code : "network",
        });
      }
    }
    return failed ? null : paths;
  }
  return {
    items,
    limit,
    refused,
    add,
    remove,
    clear,
    uploadHeld,
    paths: items.flatMap((item) =>
      item.status === "done" && item.path ? [item.path] : [],
    ),
    uploading: items.some((item) => item.status === "uploading"),
    failed: items.some((item) => item.status === "failed"),
  };
}
