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

/** Labels at least this many screen pixels apart along a ruler. */
const LABEL_GAP = 56;

const isWhole = (x: number) => Math.abs(x - Math.round(x)) < 1e-6;

/**
 * The ruler's tick spacing (`minor`, the visible grid step) and label spacing
 * (`major`, a whole multiple of it). Labels prefer round numbers (1, 2, 2.5, 5
 * × 10ⁿ); for steps those can't divide, like 0.3 or 1/3, they use the
 * roundest-looking multiple of the step (3, 3.5, 2…).
 */
export function rulerSteps(step: number, pixelsPerUnit: number): { minor: number; major: number } {
  const minor = visibleStep(step, pixelsPerUnit);
  for (let exp = -3; exp <= 9; exp++) {
    for (const m of [1, 2, 2.5, 5]) {
      const major = m * 10 ** exp;
      if (major * pixelsPerUnit >= LABEL_GAP && major >= minor && isWhole(major / minor)) return { minor, major };
    }
  }
  // Odd steps: the roundest-looking multiple that isn't too sparse — halves
  // and whole numbers first, then one decimal, then two.
  for (const scale of [2, 10, 100]) {
    for (let k = 1; minor * k * pixelsPerUnit <= LABEL_GAP * 3; k++) {
      const major = minor * k;
      if (major * pixelsPerUnit >= LABEL_GAP && isWhole(major * scale)) return { minor, major };
    }
  }
  let k = 1;
  while (minor * k * pixelsPerUnit < LABEL_GAP) k *= 2;
  return { minor, major: minor * k };
}

/** Grid steps the toolbar accepts, in canvas units. */
export const GRID_STEP_MIN = 0.01;
export const GRID_STEP_MAX = 1000;

/** Reads a typed grid step: a number (`0.3`) or a fraction (`1/3`). Undefined if unusable. */
export function parseStep(text: string): number | undefined {
  const fraction = /^\s*(\d*\.?\d+)\s*\/\s*(\d*\.?\d+)\s*$/.exec(text);
  const value = fraction ? Number(fraction[1]) / Number(fraction[2]) : Number(text.trim());
  return Number.isFinite(value) && value >= GRID_STEP_MIN && value <= GRID_STEP_MAX ? value : undefined;
}

/** Shows a grid step as typed: `0.25`, or `1/3` for values a short decimal can't show. */
export function formatStep(value: number): string {
  if (isWhole(value * 100)) return String(Math.round(value * 100) / 100);
  for (let k = 2; k <= 64; k++) {
    if (isWhole(value * k)) return `${Math.round(value * k)}/${k}`;
  }
  return String(Math.round(value * 1e4) / 1e4);
}
