import { NotFound as LazyNotFound } from "./providers";

/**
 * Every address with no page, inside the root layout, so the theme and language scripts and providers apply. Next puts
 * the root not-found into every page's error boundary, so whatever it loads, every page loads: the page itself is lazy
 * (see `NotFound` in providers.tsx).
 */
export default function NotFound() {
  return <LazyNotFound />;
}
