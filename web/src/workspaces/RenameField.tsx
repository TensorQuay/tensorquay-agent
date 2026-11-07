"use client";

import { useT } from "../i18n/I18nProvider";
import { InlineEdit } from "../ui/InlineEdit";

/** How a workspace is renamed in place: saving the new name (false keeps editing) and ending the edit. */
export type Rename = {
  onCommit: (name: string) => Promise<boolean>;
  onEnd: () => void;
};

/** A workspace's name as the In-place edit (the UI standard): the name itself becomes the field. */
export function RenameField({
  name,
  rename,
}: {
  name: string;
  rename: Rename;
}) {
  const t = useT();
  return (
    <InlineEdit
      value={name}
      label={t("workspaces.nameLabel")}
      onCancel={rename.onEnd}
      onCommit={async (value) => {
        const saved = await rename.onCommit(value);
        if (saved) rename.onEnd();
        return saved;
      }}
    />
  );
}
