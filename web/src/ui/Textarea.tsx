"use client";

import { useId, type ComponentPropsWithRef } from "react";

import { Field } from "./Field";

/** A labelled native textarea that associates and announces validation errors. */
export function Textarea({
  label,
  labelHidden = false,
  error,
  id: suppliedId,
  className = "",
  "aria-describedby": describedBy,
  ...props
}: ComponentPropsWithRef<"textarea"> & {
  label: string;
  labelHidden?: boolean;
  error?: string;
}) {
  const generatedId = useId();
  const id = suppliedId ?? generatedId;
  return (
    <Field id={id} label={label} labelHidden={labelHidden} error={error}>
      <textarea
        rows={1}
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
    </Field>
  );
}
