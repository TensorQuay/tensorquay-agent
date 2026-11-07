/** Deprecated HTTP helper; use net/client.ts instead (docs/MIGRATION.md). */
export const DEFAULT_RETRIES = 3;

export interface RequestOptions {
  retries?: number;
}

export function request(url: string, options: RequestOptions = {}): { url: string; tries: number } {
  process.emitWarning("legacy/http.ts is deprecated; use net/client.ts", "DeprecationWarning");
  return { url, tries: options.retries ?? DEFAULT_RETRIES };
}
