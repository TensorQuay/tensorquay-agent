"use client";

import "./catalogue";
import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { MenuButton } from "../ui/MenuButton";

import type { WorkspaceFilter, WorkspaceSort } from "./use-workspaces";
import "../i18n/deferred-catalogue";

export const FILTERS: readonly WorkspaceFilter[] = [
  "all",
  "in_use",
  "idle",
  "shared_with_me",
];
const FILTER_LABELS = {
  all: "workspaces.filterAll",
  in_use: "workspaces.filterInUse",
  idle: "workspaces.filterIdle",
  shared_with_me: "workspaces.filterShared",
} as const satisfies Record<WorkspaceFilter, string>;

/** The Workspaces page's search, filter and sort (UI-GUIDELINES §7): each maps to the gateway's list (GW-I-117, GW-I-122). */
export function WorkspacesToolbar({
  search,
  onSearch,
  filter,
  onFilter,
  sort,
  onSort,
}: {
  search: string;
  onSearch: (value: string) => void;
  filter: WorkspaceFilter;
  onFilter: (value: WorkspaceFilter) => void;
  sort: WorkspaceSort;
  onSort: (value: WorkspaceSort) => void;
}) {
  const t = useT();
  return (
    <div className="workspaces-toolbar">
      <input
        type="search"
        className="form-control workspaces-search"
        aria-label={t("workspaces.search")}
        placeholder={t("workspaces.search")}
        autoComplete="off"
        value={search}
        onChange={(event) => onSearch(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape" && search) {
            event.preventDefault();
            onSearch("");
          }
        }}
      />
      <div
        className="workspace-filters segmented"
        role="group"
        aria-label={t("workspaces.filterLabel")}
      >
        {FILTERS.map((value) => (
          <Button
            key={value}
            variant="ghost"
            aria-pressed={filter === value}
            onClick={() => onFilter(value)}
          >
            {t(FILTER_LABELS[value])}
          </Button>
        ))}
      </div>
      {/* The standard picker (WEB-U-134), named like the search beside it, so the toolbar stays one line. */}
      <MenuButton
        label={t("workspaces.sort")}
        placement="below-end"
        value={sort}
        options={[
          { id: "recent", label: t("workspaces.sortRecent") },
          { id: "name", label: t("workspaces.sortName") },
        ]}
        onChange={(value) => onSort(value === "name" ? "name" : "recent")}
      />
    </div>
  );
}
