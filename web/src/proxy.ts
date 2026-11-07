import { NextResponse, type NextRequest } from "next/server";

/**
 * The app's Content-Security-Policy (the CSP design): a fresh nonce for every page, which Next.js stamps on its own
 * scripts and the root layout on its two inline ones. The proxy reads nothing from the request but copies its headers,
 * and always sets x-nonce itself, so a client can't choose it: the nonce is the only per-request value a page holds
 * (scripts/server-boundary.mjs names this file). No 'unsafe-eval' (WEB-U-008); style attributes need 'unsafe-inline'.
 */
export function policy(nonce: string) {
  return [
    "default-src 'self'",
    // copy-lint: not user copy
    `script-src 'self' 'nonce-${nonce}' 'strict-dynamic'`,
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: blob:",
    "font-src 'self'",
    "connect-src 'self'",
    // The previews' frame (/files/view, its own policy) and pdf.js's worker, both from this origin.
    "frame-src 'self'",
    "worker-src 'self' blob:",
    "object-src 'none'",
    "base-uri 'none'",
    "form-action 'self'",
    "frame-ancestors 'none'",
  ].join("; ");
}

export function proxy(request: NextRequest) {
  const nonce = Buffer.from(crypto.randomUUID()).toString("base64");
  const value = policy(nonce);
  const headers = new Headers(request.headers);
  headers.set("x-nonce", nonce);
  headers.set("content-security-policy", value);
  const response = NextResponse.next({ request: { headers } });
  response.headers.set("content-security-policy", value);
  return response;
}

// Pages only: not static files, and not link prefetches, which carry no document of their own.
export const config = {
  matcher: [
    {
      source: "/((?!_next/static|_next/image|brand|pdfjs|favicon.ico).*)",
      missing: [
        { type: "header", key: "next-router-prefetch" },
        { type: "header", key: "purpose", value: "prefetch" },
      ],
    },
  ],
};
