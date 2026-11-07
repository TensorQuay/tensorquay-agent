/** One page of reads: 50 at a time, older than `before` (the previous page's `next`) when given (vault slice 2). */
export function readsQuery(
  before?: string,
  extra: Record<string, string> = {},
) {
  return new URLSearchParams({
    ...extra,
    limit: "50",
    ...(before ? { before } : {}),
  });
}
