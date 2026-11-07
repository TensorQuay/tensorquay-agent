"use client";

import { useState } from "react";

import { PasswordField } from "../auth/PasswordField";
import { ThemeSelect } from "../shell/ThemeSelect";

function PasswordExample({
  initial,
  disabled = false,
}: {
  initial: string;
  disabled?: boolean;
}) {
  const [value, setValue] = useState(initial);
  return (
    <PasswordField
      email="member@example.invalid"
      setup
      value={value}
      onChange={setValue}
      disabled={disabled}
    />
  );
}

/** Keeps setup rules, visibility controls and account preferences reviewable in both themes. */
export function AccountPatterns({ theme }: { theme: "light" | "dark" }) {
  return (
    <section
      className="design-panel stack"
      data-theme={theme}
      aria-label={`${theme} account patterns`}
    >
      <h2 className="type-heading">Account patterns · {theme}</h2>
      <PasswordExample initial="" />
      <PasswordExample initial="short" />
      <PasswordExample initial="synthetic-example-password" />
      <PasswordExample initial="synthetic-example-password" disabled />
      <ThemeSelect />
    </section>
  );
}
