"use client";

import { useQuery } from "@tanstack/react-query";

import { api } from "../api/client";
import { profiles } from "../api/profiles";

/** The profiles a new task can start from: the caller's own, then the shared ones, usable ones only (ADR-0012). */
export function useProfiles(enabled = true) {
  const query = useQuery({
    queryKey: ["profiles"],
    queryFn: ({ signal }) => profiles(api, signal),
    enabled,
    staleTime: 60_000,
    retry: false,
  });
  return {
    ...query,
    usable: (query.data?.items ?? []).filter((item) => item.usable),
  };
}
