"use client";

import { useQuery } from "@tanstack/react-query";

import { api } from "../api/client";

/** Shares the authenticated model catalogue between new and existing conversations. */
export const useModels = (enabled = true) =>
  useQuery({
    queryKey: ["models"],
    queryFn: ({ signal }) => api.models(signal),
    enabled,
    staleTime: 60_000,
    retry: false,
  });
