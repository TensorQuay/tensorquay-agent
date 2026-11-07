import { redirect } from "next/navigation";

// This section moved (WEB-U-075); its old address sends the reader on.
export default function ConnectorCredentials() {
  redirect("/settings/keys");
}
