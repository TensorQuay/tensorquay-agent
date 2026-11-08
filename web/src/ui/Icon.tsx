import type { LucideIcon } from "lucide-react";

/**
 * A decorative icon: 16 px (20 px in the app header) with the shared 1.5 px stroke (UI-GUIDELINES §5b); the composer
 * bar's round buttons draw theirs at 2 px, so a 20 px arrow reads in a 36 px disc (§7 Composer).
 */
export function Icon({
  glyph: Glyph,
  size = 16,
  stroke = 1.5,
  className,
}: {
  glyph: LucideIcon;
  size?: 16 | 20;
  stroke?: 1.5 | 2;
  className?: string;
}) {
  return (
    <Glyph
      size={size}
      strokeWidth={stroke}
      aria-hidden="true"
      className={className}
    />
  );
}
