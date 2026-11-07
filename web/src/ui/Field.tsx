import type { ReactNode } from "react";

/** Connects a field label and optional announced error to a stable control id. */
export function Field({
  id,
  label,
  error,
  children,
}: {
  id: string;
  label: string;
  error: string | undefined;
  children: ReactNode;
}) {
  return (
    <div className="field">
      <label htmlFor={id} className="type-small">
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
