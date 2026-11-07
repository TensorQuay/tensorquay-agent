"use client";

import { useParams } from "next/navigation";

import { JoinForm } from "../../../join/JoinForm";

export default function Join() {
  const { token } = useParams<{ token: string }>();
  return (
    <main id="main" className="login-page">
      <JoinForm token={decodeURIComponent(token)} />
    </main>
  );
}
