// Made-up key bodies for tests, as the gateway's key-shape tests make them (crates/tqa-types/src/key_shapes_tests.rs,
// xorshift on 64 bits), so no test source holds anything key-shaped for the secrets scan (G-10).

export const BASE62 =
  "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
export const HEX = "0123456789abcdef";
export const UPPER_DIGIT = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const [ONE, SEVEN, THIRTEEN, SEVENTEEN] = [1, 7, 13, 17].map(BigInt) as [
  bigint,
  bigint,
  bigint,
  bigint,
];
const MASK = (ONE << BigInt(64)) - ONE;

/** A made-up key body, as the gateway's tests make it (xorshift on 64 bits), so nothing key-shaped is in the source. */
export function body(seed: number, length: number, alphabet: string): string {
  let state = BigInt(seed) | ONE;
  let out = "";
  for (let index = 0; index < length; index++) {
    state ^= (state << THIRTEEN) & MASK;
    state ^= state >> SEVEN;
    state ^= (state << SEVENTEEN) & MASK;
    out += alphabet[Number(state % BigInt(alphabet.length))];
  }
  return out;
}

/** A body in groups of 7 joined by -, so only a prefix rule can find it. */
export function grouped(
  seed: number,
  length: number,
  alphabet: string,
): string {
  const out = [...body(seed, length, alphabet)];
  for (let index = 7; index < out.length; index += 8) out[index] = "-";
  return out.join("");
}

/** A key-shaped name that no rule can mistake: 40 random letters and digits. */
export const madeUpKey = (seed = 7) => body(seed, 40, BASE62);
