"use client";

import { useEffect, useState, type RefObject } from "react";

import { contrastRatio, parseColor, tint } from "./contrast";

const pairs = [
  ["paper", "ink"],
  ["surface", "ink"],
  ["surface-raised", "ink"],
  ["line", "paper"],
  ["ink", "paper"],
  ["muted", "surface"],
  ["accent", "paper"],
  ["ok", "tint"],
  ["warn", "tint"],
  ["danger", "tint"],
  ["info", "tint"],
  ["selection", "ink"],
  ["selected-fill", "ink"],
  ["focus", "paper"],
  ["code-bg", "ink"],
] as const;

/** A rendered token and its measured contrast against the intended background. */
export type TokenValue = {
  name: string;
  value: string;
  against: string;
  ratio: number;
};

/** Reads the reference panel tokens after layout and cancels pending work on unmount. */
export function useTokenValues(ref: RefObject<HTMLElement | null>) {
  const [values, setValues] = useState<TokenValue[]>([]);
  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      if (!ref.current) return;
      const style = getComputedStyle(ref.current);
      const read = (name: string) => style.getPropertyValue(`--${name}`).trim();
      const tintAmount = Number.parseFloat(read("status-tint")) / 100;
      setValues(
        pairs.map(([name, background]) => {
          const value = read(name);
          const foreground = parseColor(value);
          const against =
            background === "tint" ? `${read("status-tint")} tint` : background;
          const color =
            background === "tint"
              ? tint(foreground, parseColor(read("surface")), tintAmount)
              : parseColor(read(background));
          return {
            name,
            value,
            against,
            ratio: contrastRatio(foreground, color),
          };
        }),
      );
    });
    return () => cancelAnimationFrame(frame);
  }, [ref]);
  return values;
}
