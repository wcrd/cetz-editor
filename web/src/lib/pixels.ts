// Shared by the grid and the rulers so their lines land on the same pixels.

/**
 * Centers a 1px line on whole pixels so it renders sharp: `round(x) + 0.5`
 * covers exactly one CSS pixel (two device pixels on a 2× screen) instead of
 * smearing across a fractional boundary.
 */
export function crisp(x: number): number {
  return Math.round(x) + 0.5;
}

/** The grid step to draw: doubled until lines are at least 8 screen pixels apart. */
export function visibleStep(step: number, pixelsPerUnit: number): number {
  while (step * pixelsPerUnit < 8) step *= 2;
  return step;
}
