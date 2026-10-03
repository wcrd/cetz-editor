/** Formats a number for Typst source like the Rust core: ≤4 decimals, no trailing zeros. */
export function num(value: number): string {
  const rounded = Math.round(value * 1e4) / 1e4;
  return String(Object.is(rounded, -0) ? 0 : rounded);
}
