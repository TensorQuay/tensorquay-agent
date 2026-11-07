"use client";

import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";

import { api } from "../api/client";

/** Shares the validated catalogue and its declared capabilities without probing a runtime. */
export function useTemplates(enabled = true) {
  const query = useQuery({
    queryKey: ["templates"],
    queryFn: ({ signal }) => api.templates(signal),
    enabled,
    staleTime: 60_000,
    retry: false,
  });
  // The same choices until the catalogue changes, so what is built from them isn't rebuilt on every render (WEB-U-151).
  const choices = useMemo(
    () =>
      (query.data?.items ?? []).map((template) => ({
        id: template.name,
        label: template.display_name,
        capabilities: template.capabilities,
        description: template.description,
        size: { cpus: template.cpus, memoryMib: template.memory_mib },
      })),
    [query.data],
  );
  return { ...query, choices };
}
