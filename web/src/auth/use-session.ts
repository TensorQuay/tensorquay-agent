"use client";

import { useQuery } from "@tanstack/react-query";

import { api } from "../api/client";

/** Shares authenticated identity between protected pages and the account control. */
export function useSession(enabled = true) {
  return useQuery({
    queryKey: ["session"],
    queryFn: ({ signal }) => api.me(signal),
    enabled,
    staleTime: 60_000,
    retry: false,
  });
}
