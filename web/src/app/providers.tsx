"use client";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { lazy, Suspense, useState, type ReactNode } from "react";

import { I18nProvider } from "../i18n/I18nProvider";

export default function Providers({
  children,
}: Readonly<{ children: ReactNode }>) {
  const [queryClient] = useState(() => new QueryClient());

  return (
    <QueryClientProvider client={queryClient}>
      <I18nProvider>{children}</I18nProvider>
    </QueryClientProvider>
  );
}

// The 404 page, loaded only when an address has no page. Next puts the root not-found into every page's error boundary:
// imported directly, the page and the sign-in copy it shows cost the conversation page 4.6 KiB, and even a lazy loader
// in its own file was a 442 B script on every page (WEB-U-014/015's budget, 16h). Here the loader rides in the
// providers' script, which every page loads anyway.
const NotFoundPage = lazy(() =>
  import("../shell/NotFoundPage").then(({ NotFoundPage }) => ({
    default: NotFoundPage,
  })),
);

export function NotFound() {
  return (
    <Suspense fallback={null}>
      <NotFoundPage />
    </Suspense>
  );
}
