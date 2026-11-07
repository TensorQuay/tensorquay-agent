"use client";

// Its copy travels with the panel, which loads when wanted (WEB-U-176, WEB-U-178).
import "./changes-catalogue";

import { Fragment } from "react";

import { useT } from "../i18n/I18nProvider";

/** The breadcrumb from the workspace root to the open folder or file; every step but the last opens its folder. */
export function FilePath({
  folder,
  file,
  onOpen,
}: {
  folder: string;
  file: string | null;
  onOpen: (folder: string) => void;
}) {
  const t = useT();
  const parts = folder ? folder.split("/") : [];
  const steps = [
    { label: t("files.root"), path: "" },
    ...parts.map((part, index) => ({
      label: part,
      path: parts.slice(0, index + 1).join("/"),
    })),
    ...(file
      ? [{ label: file.slice(file.lastIndexOf("/") + 1), path: null }]
      : []),
  ];
  return (
    <nav className="files-path" aria-label={t("files.path")}>
      {steps.map((step, index) => {
        const last = index === steps.length - 1;
        return (
          <Fragment key={`${index}-${step.label}`}>
            {index > 0 && (
              <span className="muted" aria-hidden="true">
                /
              </span>
            )}
            {last || step.path === null ? (
              <span className="files-step" aria-current="page">
                {step.label}
              </span>
            ) : (
              <button
                type="button"
                className="files-step link-button"
                onClick={() => onOpen(step.path ?? "")}
              >
                {step.label}
              </button>
            )}
          </Fragment>
        );
      })}
    </nav>
  );
}
