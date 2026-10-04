// Connectors: arrows from one shape's anchor to another's, straight or
// elbowed (see crates/scene/src/route.rs). This picks which anchors an
// arrow joins and previews its path; CeTZ routes the real one.

import type { Route } from "./scene";

type Point = [number, number];
/** A box on the page (y down). */
export type Box = { x0: number; y0: number; x1: number; y1: number };

const SIDES = ["north", "south", "east", "west"];
/** The anchors a connector can join, the ones on a shape's outline. */
export const COMPASS = ["east", "north-east", "north", "north-west", "west", "south-west", "south", "south-east"];

/** A connector's icon, a line with a circle at each end: stepped for elbows, diagonal for straight. */
export const ROUTE_ICONS: Record<Route, string> = {
  elbow: "M7 6h5v12h5M7 6a2 2 0 1 1-4 0a2 2 0 1 1 4 0M21 18a2 2 0 1 1-4 0a2 2 0 1 1 4 0",
  straight: "M6.4 17.6L17.6 6.4M7 19a2 2 0 1 1-4 0a2 2 0 1 1 4 0M21 5a2 2 0 1 1-4 0a2 2 0 1 1 4 0",
};

/** The shape a connector's end (`"g.a.south"`) names, without its anchor (`g.a`). */
export function endShape(anchor: string): string {
  return anchor.includes(".") ? anchor.slice(0, anchor.lastIndexOf(".")) : anchor;
}

const center = (b: Box): Point => [(b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2];

/**
 * The anchor of a shape (its page `box` and `anchors`) a connector toward
 * `other` should join. An elbow leaves square to the side facing the other
 * shape across the wider gap; a straight arrow leaves from the compass
 * anchor nearest where the line between the two centres crosses the box.
 */
export function pickAnchor(box: Box, anchors: Record<string, Point>, other: Box, route: Route): string | undefined {
  const [cx, cy] = center(box);
  const [ox, oy] = center(other);
  if (route === "straight") {
    const [dx, dy] = [ox - cx, oy - cy];
    const t = Math.min(dx ? (box.x1 - box.x0) / 2 / Math.abs(dx) : Infinity, dy ? (box.y1 - box.y0) / 2 / Math.abs(dy) : Infinity);
    const exit: Point = Number.isFinite(t) ? [cx + dx * t, cy + dy * t] : [cx, cy];
    let best: string | undefined;
    let bestDist = Infinity;
    for (const name of COMPASS) {
      const q = anchors[name];
      const d = q && Math.hypot(q[0] - exit[0], q[1] - exit[1]);
      if (q && d < bestDist) [best, bestDist] = [name, d];
    }
    if (best) return best;
  }
  const gapX = Math.max(other.x0 - box.x1, box.x0 - other.x1);
  const gapY = Math.max(other.y0 - box.y1, box.y0 - other.y1);
  const side = gapY >= gapX ? (oy > cy ? "south" : "north") : ox > cx ? "east" : "west";
  return side in anchors ? side : SIDES.find((s) => s in anchors);
}

/** How far (canvas units) a detouring elbow steps out from each side, as `STUB` in `route.rs`. */
export const STUB = 0.5;

/** Whether an arrow leaves (or enters) this anchor up or down, as in `route.rs`. */
export function vertical(anchor: string): boolean {
  return !/(^|\.)(east|west)$/.test(anchor);
}

/** The way out of a shape through this anchor, on the page (y down), as `route.rs` steps out. */
export function outward(anchor: string): Point {
  const side = anchor.slice(anchor.lastIndexOf(".") + 1);
  if (side === "east") return [1, 0];
  if (side === "west") return [-1, 0];
  return side.includes("north") ? [0, -1] : [0, 1];
}

/**
 * Whether an elbow between these anchors (page points) needs to detour:
 * unless each end's side faces the way the plain elbow runs (out of
 * `a.south` to a shape below it, into `b.west` from its left), it would
 * double back through a shape.
 */
export function needsDetour(a: Point, aAnchor: string, b: Point, bAnchor: string): boolean {
  const [da, db] = [outward(aAnchor), outward(bAnchor)];
  const along = (from: Point, to: Point, d: Point) => (to[0] - from[0]) * d[0] + (to[1] - from[1]) * d[1];
  const eps = 1e-6;
  if (vertical(aAnchor) === vertical(bAnchor)) return !(along(a, b, da) > eps && along(b, a, db) > eps);
  const corner: Point = vertical(aAnchor) ? [a[0], b[1]] : [b[0], a[1]];
  return !(along(a, corner, da) > eps && along(b, corner, db) > eps);
}

/**
 * The page path of a connector between two anchors, as CeTZ will route it:
 * its middle segment `bend` of the way along, and a detour stepping `stub`
 * (page units) out from each side when it needs one.
 */
export function routePoints(a: Point, aAnchor: string, b: Point, bAnchor: string, route: Route, { bend = 0.5, stub = 0 } = {}): Point[] {
  if (route === "straight") return [a, b];
  const [va, vb] = [vertical(aAnchor), vertical(bAnchor)];
  const m: Point = [a[0] + bend * (b[0] - a[0]), a[1] + bend * (b[1] - a[1])];
  if (stub > 0 && needsDetour(a, aAnchor, b, bAnchor)) {
    const [da, db] = [outward(aAnchor), outward(bAnchor)];
    const sa: Point = [a[0] + da[0] * stub, a[1] + da[1] * stub];
    const sb: Point = [b[0] + db[0] * stub, b[1] + db[1] * stub];
    if (va && vb) return [a, sa, [m[0], sa[1]], [m[0], sb[1]], sb, b];
    if (!va && !vb) return [a, sa, [sa[0], m[1]], [sb[0], m[1]], sb, b];
    return [a, sa, va ? [sb[0], sa[1]] : [sa[0], sb[1]], sb, b];
  }
  if (va && vb) return [a, [a[0], m[1]], [b[0], m[1]], b];
  if (!va && !vb) return [a, [m[0], a[1]], [m[0], b[1]], b];
  return va ? [a, [a[0], b[1]], b] : [a, [b[0], a[1]], b];
}
