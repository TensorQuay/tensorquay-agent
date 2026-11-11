import type { ReactNode } from "react";

/**
 * Connects a field label and optional announced error to a stable control id. With `labelHidden`, where the place
 * says what the field is for (a placeholder), the label is for screen readers only and still names the control.
 */
export function Field({
  id,
  label,
  labelHidden = false,
  error,
  children,
}: {
  id: string;
  label: string;
  labelHidden?: boolean;
  error: string | undefined;
  children: ReactNode;
}) {
  return (
    <div className="field">
      <label htmlFor={id} className={labelHidden ? "sr-only" : "type-small"}>
        {label}
      </label>
      {children}
      {error && (
        <p id={`${id}-error`} role="alert" className="field-error type-small">
          {error}
        </p>
      )}
    </div>
  );
}
