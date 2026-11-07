"use client";

import { useRef } from "react";

import { ButtonsAndFields } from "./ButtonsAndFields";
import { ComponentGallery } from "./ComponentGallery";
import { TokenPalette } from "./TokenPalette";
import { TypeAndSpace } from "./TypeAndSpace";
import { useTokenValues } from "./use-token-values";

/** Renders one explicitly themed visual reference independent of the surrounding shell. */
export function DesignPanel({ theme }: { theme: "light" | "dark" }) {
  const ref = useRef<HTMLElement>(null);
  const values = useTokenValues(ref);
  return (
    <section
      ref={ref}
      className="design-panel"
      data-theme={theme}
      data-testid={`design-${theme}`}
      data-ready={values.length > 0}
      aria-label={`${theme} reference`}
    >
      <header>
        <h2 className="type-heading">{theme === "light" ? "Light" : "Dark"}</h2>
        <span className="brand-mark" aria-hidden="true" />
      </header>
      <TokenPalette values={values} />
      <TypeAndSpace />
      <ButtonsAndFields />
      <ComponentGallery />
    </section>
  );
}
