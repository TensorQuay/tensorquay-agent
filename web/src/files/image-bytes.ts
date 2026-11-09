import { useQuery } from "@tanstack/react-query";

import { api } from "../api/client";

/** An image larger than this isn't fetched to be drawn: the Files panel's preview and a sent message's picture. */
export const MAX_IMAGE_BYTES = 10 * 1024 * 1024;

/**
 * A workspace image's bytes, read once per version (its etag) into the one cache everything that draws it shares (the
 * Files panel's preview, a sent message's picture, WEB-U-464): never before its size is known, nor above
 * MAX_IMAGE_BYTES.
 */
export function useImageBytes(
  conversationId: string,
  path: string,
  file: { size: number; etag: string } | undefined,
) {
  return useQuery({
    queryKey: ["file-image", conversationId, path, file?.etag],
    queryFn: async ({ signal }) =>
      (await api.fileBlob(conversationId, path, signal)).arrayBuffer(),
    enabled: file !== undefined && file.size <= MAX_IMAGE_BYTES,
    retry: false,
  });
}
