"use client";

import { SettingsShell } from "../../../../settings/SettingsShell";
import { TokensSettings } from "../../../../settings/TokensSettings";

export default function Tokens() {
  return (
    <SettingsShell section="tokens">{() => <TokensSettings />}</SettingsShell>
  );
}
