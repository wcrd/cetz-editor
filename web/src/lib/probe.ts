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
