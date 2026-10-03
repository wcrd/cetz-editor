// Geometry recorded by the CeTZ probe (crates/compile/src/probe.typ).
// Coordinates are CeTZ canvas units (y up) unless converted with `toPage`.

export type Vec3 = [number, number, number];

/** A CeTZ path segment: straight lines through points, or one cubic Bézier. */
export type Segment = ["l", ...Vec3[]] | ["c", Vec3, Vec3, Vec3];

/** `[origin, closed, segments]`, one subpath of a CeTZ path drawable. */
export type SubPath = [Vec3, boolean, Segment[]];

export type ProbeDrawable =
  | { type: "path"; segments: SubPath[] }
  | { type: "content"; pos: Vec3; width: number; height: number };

export interface Probe {
  /** Byte offset of the draw call in the original source. */
  id: number;
  name: string | null;
  drawables: ProbeDrawable[];
  anchors: Record<string, unknown>;
  /** CeTZ's transform for the call's own coordinates (4x4, row-major rows). */
  transform: number[][];
  /** Points per canvas unit. */
  length: number;
  /** Where canvas (0, 0) landed on the page, in points. */
  origin: { page: number; x: number; y: number };
}

export function toPage(probe: Probe, [x, y]: Vec3 | number[]): [number, number] {
  return [probe.origin.x + x * probe.length, probe.origin.y - y * probe.length];
}

export function isVec(value: unknown): value is Vec3 {
  return Array.isArray(value) && value.length >= 2 && value.every((v) => typeof v === "number");
}

/** SVG path data for a drawable, in page points. */
export function pathData(probe: Probe, d: ProbeDrawable): string {
  const pt = (p: Vec3) => toPage(probe, p).map((n) => n.toFixed(3)).join(",");
  if (d.type === "content") {
    const [cx, cy] = toPage(probe, d.pos);
    const w = d.width * probe.length;
    const h = d.height * probe.length;
    return `M${cx - w / 2},${cy - h / 2}h${w}v${h}h${-w}Z`;
  }
  return d.segments
    .map(([origin, closed, segments]) => {
      let out = `M${pt(origin)}`;
      for (const [kind, ...points] of segments) {
        out += kind === "l" ? points.map((p) => ` L${pt(p as Vec3)}`).join("") : ` C${points.map((p) => pt(p as Vec3)).join(" ")}`;
      }
      return closed ? `${out} Z` : out;
    })
    .join(" ");
}

/** Applies a probe's transform to a point in the call's own coordinates. */
export function transformPoint(m: number[][] | undefined, [x, y]: [number, number]): [number, number] {
  if (!m) return [x, y];
  return [m[0][0] * x + m[0][1] * y + m[0][3], m[1][0] * x + m[1][1] * y + m[1][3]];
}

/** Maps a canvas-space delta back into the call's own coordinates. */
export function untransformDelta(m: number[][] | undefined, [dx, dy]: [number, number]): [number, number] {
  if (!m) return [dx, dy];
  const [a, b, c, d] = [m[0][0], m[0][1], m[1][0], m[1][1]];
  const det = a * d - b * c;
  if (Math.abs(det) < 1e-12) return [dx, dy];
  return [(d * dx - b * dy) / det, (-c * dx + a * dy) / det];
}

/** The axis-aligned bounds of a probe's drawables, in canvas units. */
export function probeBounds(probe: Probe): { x0: number; y0: number; x1: number; y1: number } | undefined {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  const add = ([x, y]: Vec3) => {
    x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y);
  };
  for (const d of probe.drawables) {
    if (d.type === "content") {
      add([d.pos[0] - d.width / 2, d.pos[1] - d.height / 2, 0]);
      add([d.pos[0] + d.width / 2, d.pos[1] + d.height / 2, 0]);
    } else {
      for (const [origin, , segments] of d.segments) {
        add(origin);
        for (const [, ...points] of segments) points.forEach((p) => add(p as Vec3));
      }
    }
  }
  return x0 <= x1 ? { x0, y0, x1, y1 } : undefined;
}
