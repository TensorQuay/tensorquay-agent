import type { LucideIcon } from "lucide-react";

/** A decorative icon: 16 px (20 px in the app header) with the shared 1.5 px stroke (UI-GUIDELINES §5b). */
export function Icon({
  glyph: Glyph,
  size = 16,
  className,
}: {
  glyph: LucideIcon;
  size?: 16 | 20;
  className?: string;
}) {
  return (
    <Glyph
      size={size}
      strokeWidth={1.5}
      aria-hidden="true"
      className={className}
    />
  );
}
