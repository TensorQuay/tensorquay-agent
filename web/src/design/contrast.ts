/** An sRGB colour expressed as channels from zero to 255. */
export type RGB = readonly [number, number, number];

/** Reads full or shorthand hex tokens; throws for unsupported colour syntax. */
export function parseColor(value: string): RGB {
  const digits = value.match(/^#([\da-f]{3}|[\da-f]{6})$/i)?.[1];
  if (!digits) throw new Error(`Unsupported token colour: ${value}`);
  const hex =
    digits.length === 3
      ? Array.from(digits, (digit) => digit + digit).join("")
      : digits;
  return [
    Number.parseInt(hex.slice(0, 2), 16),
    Number.parseInt(hex.slice(2, 4), 16),
    Number.parseInt(hex.slice(4, 6), 16),
  ];
}

function luminance(color: RGB) {
  const linear = (channel: number) => {
    const value = channel / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  };
  return (
    linear(color[0]) * 0.2126 +
    linear(color[1]) * 0.7152 +
    linear(color[2]) * 0.0722
  );
}

/** Computes WCAG relative-luminance contrast for two sRGB colours. */
export function contrastRatio(foreground: RGB, background: RGB) {
  const a = luminance(foreground);
  const b = luminance(background);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

/** Composites a colour over its background using a fractional opacity. */
export function tint(foreground: RGB, background: RGB, amount: number): RGB {
  const channel = (index: 0 | 1 | 2) =>
    foreground[index] * amount + background[index] * (1 - amount);
  return [channel(0), channel(1), channel(2)];
}
