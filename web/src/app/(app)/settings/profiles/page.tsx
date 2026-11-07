import { redirect } from "next/navigation";

// Task profiles are Agent profiles now; the old address sends the reader on.
export default function Profiles() {
  redirect("/settings/agent-profiles");
}
