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

/** Whether an arrow leaves (or enters) this anchor up or down, as in `route.rs`. */
function vertical(anchor: string): boolean {
  return !/(^|\.)(east|west)$/.test(anchor);
}

/** The page path of a connector between two anchors, as CeTZ will route it. */
export function routePoints(a: Point, aAnchor: string, b: Point, bAnchor: string, route: Route): Point[] {
  if (route === "straight") return [a, b];
  const [va, vb] = [vertical(aAnchor), vertical(bAnchor)];
  if (va && vb) {
    const y = (a[1] + b[1]) / 2;
    return [a, [a[0], y], [b[0], y], b];
  }
  if (!va && !vb) {
    const x = (a[0] + b[0]) / 2;
    return [a, [x, a[1]], [x, b[1]], b];
  }
  return va ? [a, [a[0], b[1]], b] : [a, [b[0], a[1]], b];
}
