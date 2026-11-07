/** The HTTP client that replaces legacy/http.ts. */
export const DEFAULT_ATTEMPTS = 3;

export interface SendOptions {
  url: string;
  attempts?: number;
}

export function send(options: SendOptions): { url: string; tries: number } {
  return { url: options.url, tries: options.attempts ?? DEFAULT_ATTEMPTS };
}
