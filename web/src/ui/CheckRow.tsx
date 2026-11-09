"use client";

import {
  Circle,
  CircleDot,
  Square,
  SquareCheck,
  SquareMinus,
} from "lucide-react";
import {
  useEffect,
  useRef,
  type InputHTMLAttributes,
  type ReactNode,
} from "react";

import { Icon } from "./Icon";

/**
 * The standard check row (UI-GUIDELINES §7 Checkbox, WEB-U-434): a checkbox or radio and its words, the whole row its
 * label. The native control stays, for its keyboard, its label and its form, filling a 32 px target, transparent, and
 * the standard 16 px box (a circle for a radio) is drawn under it. Its box follows `checked`, so it is controlled.
 */
export function CheckRow({
  type = "checkbox",
  label,
  className,
  mixed = false,
  ...input
}: Omit<InputHTMLAttributes<HTMLInputElement>, "type"> & {
  type?: "checkbox" | "radio";
  /** Its words (and anything shown with them), after the box. */
  label: ReactNode;
  /** The row's own class beside check-row, such as its type size. */
  className?: string | undefined;
  /** Some of a group are on (All tools): the box shows a minus and the input says it is mixed. */
  mixed?: boolean;
}) {
  const own = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (own.current) own.current.indeterminate = mixed;
  }, [mixed]);
  const [off, on] =
    type === "radio" ? [Circle, CircleDot] : [Square, SquareCheck];
  return (
    <label className={["check-row", className].filter(Boolean).join(" ")}>
      <span className="check-control">
        <input type={type} {...input} ref={own} />
        <Icon
          glyph={mixed ? SquareMinus : input.checked ? on : off}
          size={16}
        />
      </span>
      {label}
    </label>
  );
}
