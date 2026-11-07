"use client";

import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useEffect, useState, type ComponentProps } from "react";

import { HostKeyPage } from "./HostKeyPage";

/** Longer than any web address a key can have (a host of up to 253 characters and a path). */
const LONGEST = 1024;

/**
 * Add a key, opened for an address when a task links to `?address=` ("Add a key for {host}", WEB-U-148). The address
 * comes from the router, never window.location, which still shows the page the person came from while this one first
 * renders on a client navigation (the review of f8780321, item 1). It is taken off the page's address once read, so a
 * reload or Back doesn't ask again; the page checks it as it checks anything typed. The route that renders this wraps
 * it in Suspense, as useSearchParams needs.
 */
export function NewHostKeyPage(
  props: Omit<ComponentProps<typeof HostKeyPage>, "id" | "address">,
) {
  const params = useSearchParams();
  const router = useRouter();
  const pathname = usePathname();
  const [asked] = useState(() => {
    const address = params.get("address")?.trim() ?? "";
    return address && address.length <= LONGEST ? address : undefined;
  });
  useEffect(() => {
    if (params.has("address")) router.replace(pathname);
  }, [params, router, pathname]);
  return <HostKeyPage {...props} {...(asked ? { address: asked } : {})} />;
}
