import { parseExpr } from "./props";

// The options each CeTZ 0.5.2 draw function takes (its named arguments and the
// style keys it reads, from `draw/shapes.typ` and `styles.typ`), so the
// inspector can list them all, set or not. Defaults are CeTZ's own.

export type OptKind =
  | { kind: "color" }
  | { kind: "stroke" }
  | { kind: "mark" }
  /** `options` are source texts: `"rect"`, `none`. */
  | { kind: "choice"; options: string[] }
  | { kind: "number"; step: number; unit?: string }
  | { kind: "length" }
  | { kind: "bool" }
  | { kind: "code" };

export type Opt = OptKind & {
  key: string;
  /** What CeTZ uses when the option isn't given, as source. */
  default: string;
  help: string;
};

export type Group = { title: string; opts: Opt[] };

const COMPASS = ["center", "north", "south", "east", "west", "north-east", "north-west", "south-east", "south-west"];
const quoted = (xs: string[]) => xs.map((x) => JSON.stringify(x));

const anchor = (names = COMPASS): Opt => ({
  key: "anchor",
  kind: "choice",
  options: quoted(names),
  default: "none",
  help: "Which of its anchors sits at the position",
});
const angle: Opt = { key: "angle", kind: "number", step: 15, unit: "deg", default: "0deg", help: "Rotation" };
const close: Opt = { key: "close", kind: "bool", default: "false", help: "Join the last point back to the first" };
const radius = (def: string): Opt => ({ key: "radius", kind: "number", step: 0.1, default: def, help: "Radius in canvas units, or (x, y)" });

const stroke: Opt = { key: "stroke", kind: "stroke", default: "1pt + black", help: "Outline: color, thickness, dash" };
const fill: Opt = { key: "fill", kind: "color", default: "none", help: "Fill color" };
const fillRule: Opt = { key: "fill-rule", kind: "choice", options: quoted(["non-zero", "even-odd"]), default: '"non-zero"', help: "Which areas of a self-crossing path are filled" };
const mark: Opt = { key: "mark", kind: "mark", default: "none", help: "Arrow heads and other marks at the ends" };

const shape = (title: string, opts: Opt[]): Group => ({ title, opts });
const style = (...opts: Opt[]): Group => ({ title: "Style", opts });

const PATH_STYLE = style(stroke, mark, fill, fillRule);

const SCHEMA: Record<string, Group[]> = {
  line: [shape("Shape", [close]), PATH_STYLE],
  bezier: [PATH_STYLE],
  "bezier-through": [PATH_STYLE],
  catmull: [
    shape("Shape", [close, { key: "tension", kind: "number", step: 0.1, default: "0.5", help: "How tightly the curve follows its points" }]),
    PATH_STYLE,
  ],
  hobby: [
    shape("Shape", [
      close,
      { key: "omega", kind: "code", default: "(0, 0)", help: "Curl at the start and end" },
      { key: "ta", kind: "code", default: "auto", help: "Tangent angles at each point" },
      { key: "tb", kind: "code", default: "auto", help: "Tangent angles at each point" },
    ]),
    PATH_STYLE,
  ],
  circle: [shape("Shape", [radius("1"), anchor()]), style(stroke, fill)],
  "circle-through": [shape("Shape", [anchor()]), style(stroke, fill)],
  arc: [
    shape("Shape", [
      { key: "start", kind: "number", step: 15, unit: "deg", default: "auto", help: "Start angle" },
      { key: "stop", kind: "number", step: 15, unit: "deg", default: "auto", help: "End angle" },
      { key: "delta", kind: "number", step: 15, unit: "deg", default: "auto", help: "Angle swept from the start" },
      radius("1"),
      { key: "mode", kind: "choice", options: quoted(["OPEN", "CLOSE", "PIE"]), default: '"OPEN"', help: "Leave open, close with a chord, or draw a pie slice" },
      anchor(["origin", "start", "end", "arc-start", "arc-end", "arc-center", "chord-center", "center"]),
    ]),
    style(stroke, mark, fill),
  ],
  "arc-through": [shape("Shape", [{ key: "mode", kind: "choice", options: quoted(["OPEN", "CLOSE", "PIE"]), default: '"OPEN"', help: "Leave open, close with a chord, or draw a pie slice" }]), style(stroke, mark, fill)],
  rect: [shape("Shape", [radius("0"), anchor()]), style(stroke, fill)],
  polygon: [shape("Shape", [radius("1"), angle, anchor()]), style(stroke, fill, fillRule)],
  "n-star": [
    shape("Shape", [
      radius("1"),
      { key: "inner-radius", kind: "code", default: "50%", help: "Radius of the inner points: a number or a ratio of the radius" },
      { key: "show-inner", kind: "bool", default: "false", help: "Connect the inner points" },
      angle,
      anchor(),
    ]),
    style(stroke, fill),
  ],
  grid: [
    shape("Shape", [
      { key: "step", kind: "number", step: 0.1, default: "1", help: "Spacing, or (x: .., y: ..)" },
      { key: "help-lines", kind: "bool", default: "false", help: "Thin gray lines" },
    ]),
    style(stroke),
  ],
  content: [
    shape("Layout", [
      anchor([...COMPASS, "mid", "mid-east", "mid-west", "base", "base-east", "base-west", "text"]),
      angle,
      { key: "frame", kind: "choice", options: ['"rect"', '"circle"'], default: "none", help: "Draw a frame around the content" },
      { key: "padding", kind: "number", step: 0.1, default: "0", help: "Space around the content: a number or (top: .., ..)" },
      { key: "auto-scale", kind: "bool", default: "false", help: "Scale the content with the canvas" },
    ]),
    style(fill, stroke),
  ],
  group: [shape("Layout", [anchor(), { key: "padding", kind: "number", step: 0.1, default: "0", help: "Space around the bounds" }]), style(fill, stroke)],
  "merge-path": [
    shape("Shape", [
      close,
      { key: "join", kind: "bool", default: "true", help: "Connect the parts with straight lines" },
    ]),
    style(stroke, fill, fillRule),
  ],
  mark: [
    shape("Shape", [{ key: "scale", kind: "number", step: 0.1, default: "1", help: "Size factor" }]),
    style(stroke, fill),
  ],
  "set-style": [
    style(stroke, fill, fillRule, mark, radius("1"), { key: "padding", kind: "number", step: 0.1, default: "none", help: "Default padding" }),
  ],
};

/** For calls we don't know (often your own functions): the usual style keys. */
const FALLBACK: Group[] = [style(stroke, fill)];

export function optionsFor(base: string, stateCall: boolean): Group[] {
  return SCHEMA[base] ?? (stateCall ? [] : FALLBACK);
}

/**
 * For your own function that forwards its arguments to a CeTZ one, like
 * `let face(..a) = line(..a.pos(), close: true, ..a.named())`: the options of
 * that call, minus the ones the function sets itself.
 */
export function wrappedOptions(fnSource: string): Group[] | undefined {
  const body = /^[^=]*?\)\s*=>?\s*([\s\S]*)$/.exec(fnSource)?.[1];
  const n = body && body.includes("..") ? parseExpr(body.trim()) : null;
  if (n?.kind !== "call" || n.callee.kind !== "ident" || !SCHEMA[n.callee.name]) return undefined;
  const fixed = new Set(n.args.map((a) => a.key));
  return SCHEMA[n.callee.name].map((g) => ({ ...g, opts: g.opts.filter((o) => !fixed.has(o.key)) })).filter((g) => g.opts.length > 0);
}

/** The option for a key outside a call's own list, by what that key usually is. */
export function optFor(key: string): Opt {
  for (const groups of Object.values(SCHEMA)) {
    const opt = groups.flatMap((g) => g.opts).find((o) => o.key === key);
    if (opt) return opt;
  }
  return { key, kind: "code", default: "", help: "" };
}

// --- Sub-options --------------------------------------------------------------

export const STROKE_MORE: Opt[] = [
  { key: "cap", kind: "choice", options: quoted(["butt", "round", "square"]), default: '"butt"', help: "Line ends" },
  { key: "join", kind: "choice", options: quoted(["miter", "round", "bevel"]), default: '"miter"', help: "Corners" },
  { key: "miter-limit", kind: "number", step: 0.5, default: "4", help: "How long a sharp mitered corner may get" },
];

export const MARK_MORE: Opt[] = [
  { key: "scale", kind: "number", step: 0.1, default: "1", help: "Size factor for length, width and inset" },
  { key: "length", kind: "code", default: ".2cm", help: "Size along the path" },
  { key: "width", kind: "code", default: ".15cm", help: "Size across the path" },
  { key: "anchor", kind: "choice", options: quoted(["tip", "center", "base"]), default: '"tip"', help: "Which part of the mark sits on the end" },
  { key: "harpoon", kind: "bool", default: "false", help: "Only draw one half" },
  { key: "flip", kind: "bool", default: "false", help: "Mirror across the path" },
  { key: "reverse", kind: "bool", default: "false", help: "Point the other way" },
  { key: "stroke", kind: "stroke", default: "auto", help: "Outline of the mark" },
  { key: "pos", kind: "code", default: "none", help: "Place on the path instead of at the end: a number or ratio" },
  { key: "offset", kind: "code", default: "0", help: "Extra offset along the path" },
  { key: "sep", kind: "code", default: ".1cm", help: "Gap between several marks" },
  { key: "shorten-to", kind: "code", default: "auto", help: "Which mark the path is shortened to; none to keep its length" },
];

/** The compiler's embedded fonts. */
export const FONTS = ["Libertinus Serif", "New Computer Modern", "DejaVu Sans Mono"];

export const TEXT_MORE: Opt[] = [
  { key: "font", kind: "choice", options: quoted(FONTS), default: '"Libertinus Serif"', help: "Font family" },
  {
    key: "weight",
    kind: "choice",
    options: quoted(["thin", "light", "regular", "medium", "semibold", "bold", "extrabold", "black"]),
    default: '"regular"',
    help: "Font weight",
  },
  { key: "style", kind: "choice", options: quoted(["normal", "italic", "oblique"]), default: '"normal"', help: "Font style" },
  { key: "tracking", kind: "length", default: "0pt", help: "Extra space between letters" },
  { key: "baseline", kind: "length", default: "0pt", help: "Shift up (negative) or down" },
];
