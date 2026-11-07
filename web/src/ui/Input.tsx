"use client";

import { useId, type InputHTMLAttributes, type ReactNode } from "react";

import { Field } from "./Field";

/**
 * A labelled native input that associates and announces validation errors. An adornment (such as the password's
 * show/hide button) sits inside the input's own box, so an error said under the field never moves it (WEB-U-383).
 */
export function Input({
  label,
  error,
  adornment,
  id: suppliedId,
  className = "",
  "aria-describedby": describedBy,
  ...props
}: InputHTMLAttributes<HTMLInputElement> & {
  label: string;
  error?: string;
  adornment?: ReactNode;
}) {
  const generatedId = useId();
  const id = suppliedId ?? generatedId;
  const input = (
    <input
      {...props}
      id={id}
      className={`form-control ${className}`}
      aria-invalid={error ? true : props["aria-invalid"]}
      aria-describedby={
        [describedBy, error ? `${id}-error` : undefined]
          .filter(Boolean)
          .join(" ") || undefined
      }
    />
  );
  return (
    <Field id={id} label={label} error={error}>
      {adornment ? (
        <div className="input-adorned">
          {input}
          {adornment}
        </div>
      ) : (
        input
      )}
    </Field>
  );
}
