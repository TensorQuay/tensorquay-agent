"use client";

import { useParams } from "next/navigation";

import { SettingsShell } from "../../../../../settings/SettingsShell";
import { UserTasks } from "../../../../../settings/UserTasks";

export default function Person() {
  const { id } = useParams<{ id: string }>();
  return (
    <SettingsShell section="people">
      {(me) => <UserTasks me={me} id={id} />}
    </SettingsShell>
  );
}
