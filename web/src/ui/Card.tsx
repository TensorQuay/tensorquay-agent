import type { HTMLAttributes } from "react";

/** Groups related content on a flat bordered surface. */
export function Card({
  children,
  className = "",
  ...props
}: HTMLAttributes<HTMLDivElement>) {
  return (
    <div {...props} className={`card ${className}`}>
      {children}
    </div>
  );
}
