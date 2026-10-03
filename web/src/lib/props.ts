// Structured views of argument values for the inspector: reads `text(5pt)[Hi]`,
// `1pt + red` or `(end: ">")` into fields, and writes a field change back as
// new source for that one argument. Anything not understood stays a code box.

import { expr as exprJson } from "./core";
import { OffsetIndex } from "./offsets";
import type { Range } from "./scene";

export type Node = { range: Range } & (
  | { kind: "number"; value: number }
  | { kind: "numeric"; value: number; unit: string }
  | { kind: "str"; value: string }
  | { kind: "bool"; value: boolean }
  | { kind: "none" }
  | { kind: "auto" }
  | { kind: "ident"; name: string }
  | { kind: "field"; target: Node; field: string }
  | { kind: "content"; body: Range }
  | { kind: "call"; callee: Node; args: Item[]; close: number | null }
  | { kind: "dict"; items: Item[]; close: number }
  | { kind: "array"; items: Item[]; close: number }
  | { kind: "add"; lhs: Node; rhs: Node }
  | { kind: "other" }
);

export interface Item {
  key: string | null;
  range: Range;
  value: Node;
}

/** Parses one expression; ranges are UTF-16 offsets into `text`. */
export function parseExpr(text: string): Node | null {
  const index = new OffsetIndex(text);
  return JSON.parse(exprJson(text), (key, value) => {
    if ((key === "range" || key === "body") && value && typeof value.start === "number") {
      return { start: index.toUtf16(value.start), end: index.toUtf16(value.end) };
    }
    if (key === "close" && typeof value === "number") return index.toUtf16(value);
    return value;
  });
}

const slice = (text: string, r: Range) => text.slice(r.start, r.end);

// --- Colors ----------------------------------------------------------------

/** Typst's predefined colors. */
export const NAMED_COLORS: Record<string, string> = {
  black: "#000000",
  gray: "#aaaaaa",
  silver: "#dddddd",
  white: "#ffffff",
  navy: "#001f3f",
  blue: "#0074d9",
  aqua: "#7fdbff",
  teal: "#39cccc",
  eastern: "#239dad",
  purple: "#b10dc9",
  fuchsia: "#f012be",
  maroon: "#85144b",
  red: "#ff4136",
  yellow: "#ffdc00",
  orange: "#ff851b",
  olive: "#3d9970",
  green: "#2ecc40",
  lime: "#01ff70",
};

type RGB = [number, number, number];

const hex = (c: RGB) => "#" + c.map((v) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0")).join("");

/** A 0–1 fraction from `50%`, or a 0–255 component from an integer. */
function component(n: Node | undefined): number | undefined {
  if (n?.kind === "numeric" && n.unit === "%") return (n.value / 100) * 255;
  if (n?.kind === "number") return n.value <= 1 && !Number.isInteger(n.value) ? n.value * 255 : n.value;
  return undefined;
}

function rgbOf(n: Node): RGB | undefined {
  if (n.kind === "ident") return parseHex(NAMED_COLORS[n.name]);
  if (n.kind === "field" && n.target.kind === "ident" && n.target.name === "color") return parseHex(NAMED_COLORS[n.field]);
  if (n.kind !== "call") return undefined;
  const pos = n.args.filter((a) => a.key === null).map((a) => a.value);
  const callee = n.callee;
  if (callee.kind === "ident" && callee.name === "rgb") {
    if (pos[0]?.kind === "str") return parseHex(pos[0].value);
    const c = pos.slice(0, 3).map(component);
    return c.length === 3 && c.every((v) => v !== undefined) ? (c as RGB) : undefined;
  }
  if (callee.kind === "ident" && callee.name === "luma") {
    const v = component(pos[0]);
    return v === undefined ? undefined : [v, v, v];
  }
  if (callee.kind === "field") {
    const base = rgbOf(callee.target);
    const f = pos[0]?.kind === "numeric" && pos[0].unit === "%" ? pos[0].value / 100 : undefined;
    if (!base) return undefined;
    if (callee.field === "lighten" && f !== undefined) return base.map((v) => v + (255 - v) * f) as RGB;
    if (callee.field === "darken" && f !== undefined) return base.map((v) => v * (1 - f)) as RGB;
    if (["transparentize", "opacify", "saturate", "desaturate"].includes(callee.field)) return base;
  }
  return undefined;
}

function parseHex(s: string | undefined): RGB | undefined {
  if (!s) return undefined;
  const short = /^#?([0-9a-f]{3})$/i.exec(s)?.[1];
  const m = short ? [...short].map((c) => c + c).join("") : /^#?([0-9a-f]{6})$/i.exec(s)?.[1];
  return m ? ([0, 2, 4].map((i) => parseInt(m.slice(i, i + 2), 16)) as RGB) : undefined;
}

/** The color an expression evaluates to, as `#rrggbb`, if we can tell. */
export function colorHex(n: Node | null | undefined): string | undefined {
  const c = n ? rgbOf(n) : undefined;
  return c && hex(c);
}

/** Text for a color picked in a color input. */
export function colorText(value: string): string {
  const name = Object.entries(NAMED_COLORS).find(([, v]) => v === value.toLowerCase())?.[0];
  return name ?? `rgb("${value}")`;
}

function isColor(n: Node): boolean {
  if (colorHex(n)) return true;
  if (n.kind === "call") {
    const c = n.callee;
    return (c.kind === "ident" && ["rgb", "luma", "cmyk", "oklab", "oklch", "color"].includes(c.name)) || (c.kind === "field" && isColor(c.target));
  }
  return n.kind === "field" && n.target.kind === "ident" && n.target.name === "color";
}

const LENGTH_UNITS = new Set(["pt", "mm", "cm", "in", "em"]);
const isLength = (n: Node) => n.kind === "numeric" && LENGTH_UNITS.has(n.unit);

/** Normalizes a typed length: a bare number gets `pt`. Empty → null. */
export function lengthText(input: string): string | null {
  const t = input.trim();
  if (t === "") return null;
  return /^-?\d*\.?\d+$/.test(t) ? `${t}pt` : t;
}

// --- Entries: dictionaries and argument lists ------------------------------

/** One `key: value` (or positional) piece, kept as source text. */
export interface Entry {
  key: string | null;
  text: string;
  node: Node;
}

function entries(text: string, items: Item[]): Entry[] {
  return items.map((i) => ({ key: i.key, text: slice(text, i.value.range), node: i.value }));
}

const entryText = (e: Pick<Entry, "key" | "text">) => (e.key ? `${e.key}: ${e.text}` : e.text);

/** Replaces, removes (`text: null`) or appends one entry, found by `match`. */
function setEntry(list: Entry[], match: (e: Entry) => boolean, key: string, text: string | null): Entry[] {
  const i = list.findIndex(match);
  if (text === null) return i < 0 ? list : list.filter((_, j) => j !== i);
  const node = parseExpr(text) ?? ({ kind: "other", range: { start: 0, end: text.length } } as Node);
  if (i < 0) return [...list, { key, text, node }];
  return list.map((e, j) => (j === i ? { ...e, text, node } : e));
}

const byKey = (key: string) => (e: Entry) => e.key === key;
const dictText = (list: Entry[]) => `(${list.map(entryText).join(", ")})`;

// --- Stroke ----------------------------------------------------------------

export interface Stroke {
  /** `none`: no stroke at all. */
  none: boolean;
  entries: Entry[];
}

/** Reads `1pt`, `red`, `1pt + red`, `none` or `(paint: ..., dash: ...)`; "" is unset. */
export function parseStroke(text: string): Stroke | null {
  if (text.trim() === "") return { none: false, entries: [] };
  const n = parseExpr(text);
  if (!n) return null;
  if (n.kind === "none") return { none: true, entries: [] };
  if (n.kind === "dict") return n.items.every((i) => i.key) ? { none: false, entries: entries(text, n.items) } : null;
  const parts = n.kind === "add" ? [n.lhs, n.rhs] : [n];
  const out: Entry[] = [];
  for (const p of parts) {
    const key = isLength(p) ? "thickness" : isColor(p) ? "paint" : null;
    if (!key || out.some((e) => e.key === key)) return null;
    out.push({ key, text: slice(text, p.range), node: p });
  }
  return { none: false, entries: out };
}

export function strokeGet(s: Stroke, key: string): Entry | undefined {
  return s.entries.find(byKey(key));
}

export function strokeSet(s: Stroke, key: string, text: string | null): string | null {
  const list = setEntry(s.entries, byKey(key), key, text);
  if (list.length === 0) return null;
  if (list.every((e) => e.key === "thickness" || e.key === "paint")) {
    const t = list.find(byKey("thickness"));
    const p = list.find(byKey("paint"));
    return t && p ? `${t.text} + ${p.text}` : (t ?? p)!.text;
  }
  return dictText(list);
}

export const DASHES = [
  "solid",
  "dotted",
  "densely-dotted",
  "loosely-dotted",
  "dashed",
  "densely-dashed",
  "loosely-dashed",
  "dash-dotted",
  "densely-dash-dotted",
  "loosely-dash-dotted",
];

// --- Marks -----------------------------------------------------------------

/** CeTZ mark symbols, with their short forms. */
export const MARKS: [string, string][] = [
  [">", "Triangle"],
  ["stealth", "Stealth"],
  ["straight", "Straight"],
  ["barbed", "Barbed"],
  ["|", "Bar"],
  ["o", "Circle"],
  ["<>", "Diamond"],
  ["[]", "Square"],
  ["]", "Bracket"],
  ["hook", "Hook"],
  ["x", "Cross"],
  ["+", "Plus"],
  ["*", "Star"],
  ["<", "Reversed"],
];

export type Mark = Entry[];

/** Reads a mark dictionary, `(end: ">", fill: black)`; "" is unset. */
export function parseMark(text: string): Mark | null {
  if (text.trim() === "") return [];
  const n = parseExpr(text);
  if (n?.kind !== "dict" || !n.items.every((i) => i.key)) return null;
  const list = entries(text, n.items);
  // A `symbol` applies to both ends; editing start/end separately would hide it.
  return list.some((e) => e.key === "symbol") ? null : list;
}

/** The symbol at one end, or "" for none. */
export function markEnd(m: Mark, end: "start" | "end"): string {
  const e = m.find(byKey(end));
  return e?.node.kind === "str" ? e.node.value : e ? e.text : "";
}

/** Removing the last end symbol removes the whole mark. */
export function markSet(m: Mark, key: string, text: string | null): string | null {
  const list = setEntry(m, byKey(key), key, text);
  const ends = list.some((e) => e.key === "start" || e.key === "end");
  return list.length === 0 || (!ends && (key === "start" || key === "end")) ? null : dictText(list);
}

// --- Styled text -----------------------------------------------------------

/**
 * Text content and its styling: `[body]`, `text(5pt, fill: red)[body]`,
 * `strong[body]`, `emph[...]` and nestings of those.
 */
export interface TextStyle {
  /** The markup between the brackets, and where it sits in the argument. */
  body: string;
  bodyRange: Range;
  /** Arguments of `text(...)`, except the body. */
  props: Entry[];
  strong: boolean;
  emph: boolean;
}

const WRAPPERS = new Set(["text", "strong", "emph"]);

export function parseText(text: string): TextStyle | null {
  let n = parseExpr(text);
  const style: Omit<TextStyle, "body" | "bodyRange"> = { props: [], strong: false, emph: false };
  let sawText = false;
  while (n) {
    if (n.kind === "content") return { ...style, body: slice(text, n.body), bodyRange: n.body };
    if (n.kind !== "call" || n.callee.kind !== "ident" || !WRAPPERS.has(n.callee.name)) return null;
    const name = n.callee.name;
    const pos = n.args.filter((a) => a.key === null);
    const body = pos.at(-1);
    if (!body || (body.value.kind !== "content" && body.value.kind !== "call")) return null;
    const rest = n.args.filter((a) => a !== body);
    if (name === "text") {
      if (sawText) return null;
      sawText = true;
      style.props = entries(text, rest);
    } else {
      if (rest.length > 0) return null;
      style[name as "strong" | "emph"] = true;
    }
    n = body.value;
  }
  return null;
}

const isSize = (e: Entry) => e.key === "size" || (e.key === null && isLength(e.node));
const isFill = (e: Entry) => e.key === "fill" || (e.key === null && isColor(e.node));
const isWeight = byKey("weight");
const isStyle = byKey("style");

export const textSize = (t: TextStyle) => t.props.find(isSize)?.text ?? "";
export const textFill = (t: TextStyle) => t.props.find(isFill);

export function isBold(t: TextStyle): boolean {
  if (t.strong) return true;
  const w = t.props.find(isWeight)?.node;
  return (w?.kind === "str" && ["bold", "extrabold", "black", "semibold"].includes(w.value)) || (w?.kind === "number" && w.value >= 600);
}

export function isItalic(t: TextStyle): boolean {
  const s = t.props.find(isStyle)?.node;
  return t.emph || (s?.kind === "str" && s.value !== "normal");
}

/** The argument with a new body; everything else is left as written. */
export function withBody(text: string, t: TextStyle, body: string): string {
  return text.slice(0, t.bodyRange.start) + body + text.slice(t.bodyRange.end);
}

export type TextChange =
  | { size: string | null }
  | { fill: string | null }
  | { bold: boolean }
  | { italic: boolean }
  /** Any other `text(...)` argument, by key. */
  | { prop: string; text: string | null };

/** A `text(...)` argument by key, if set. */
export const textProp = (t: TextStyle, key: string) => t.props.find(byKey(key));

/**
 * The argument with one style changed. Styling goes into `text(...)` when
 * there is one; `strong`/`emph` are used for plain bold or italic text.
 */
export function withStyle(t: TextStyle, change: TextChange): string {
  let { props, strong, emph } = t;
  if ("size" in change) props = setEntry(props, isSize, "size", change.size);
  if ("fill" in change) props = setEntry(props, isFill, "fill", change.fill);
  if ("prop" in change) {
    props = setEntry(props, byKey(change.prop), change.prop, change.text);
    // An explicit weight or style replaces strong/emph.
    if (change.prop === "weight") strong = false;
    if (change.prop === "style") emph = false;
  }
  if ("bold" in change) {
    props = setEntry(props, isWeight, "weight", null);
    strong = change.bold && props.length === 0;
    if (change.bold && !strong) props = setEntry(props, isWeight, "weight", '"bold"');
  }
  if ("italic" in change) {
    props = setEntry(props, isStyle, "style", null);
    emph = change.italic && props.length === 0;
    if (change.italic && !emph) props = setEntry(props, isStyle, "style", '"italic"');
  }
  // Once there's a text(...), fold strong/emph into it.
  if (props.length > 0) {
    if (strong && !props.some(isWeight)) props = [...props, { key: "weight", text: '"bold"', node: parseExpr('"bold"')! }];
    if (emph && !props.some(isStyle)) props = [...props, { key: "style", text: '"italic"', node: parseExpr('"italic"')! }];
    return `text(${props.map(entryText).join(", ")})[${t.body}]`;
  }
  let out = `[${t.body}]`;
  if (emph) out = `emph${out}`;
  if (strong) out = emph ? `strong(${out})` : `strong${out}`;
  return out;
}

// --- Choices ---------------------------------------------------------------

/** Source text in a comparable form: strings re-quoted, other text trimmed. */
export function canonical(text: string): string {
  const n = parseExpr(text);
  return n?.kind === "str" ? JSON.stringify(n.value) : text.trim();
}

/** A choice's label: the string without quotes. */
export function choiceLabel(text: string): string {
  const n = parseExpr(text);
  return n?.kind === "str" ? n.value : text;
}

// --- Changes across shapes ---------------------------------------------------

/**
 * What a field commits: a new value (`null` removes it), or a function of
 * each shape's current value, so a change to one part of a stroke or mark
 * keeps the rest of every selected shape's own value.
 */
export type Change = string | null | ((old: string) => string | null);

export const applyChange = (c: Change, old: string) => (typeof c === "function" ? c(old) : c);

/** The shared value of several, or `mixed` when they differ. */
export function common(texts: string[]): { text: string; mixed: boolean } {
  const mixed = texts.some((t) => t !== texts[0]);
  return { text: mixed ? "" : (texts[0] ?? ""), mixed };
}

/** One key of a stroke or mark value ("" if not given), or undefined if the value can't be read. */
export function partOf(kind: "stroke" | "mark", text: string, key: string): string | undefined {
  const entries = kind === "stroke" ? parseStroke(text)?.entries : (parseMark(text) ?? undefined);
  return entries?.find(byKey(key))?.text ?? (entries ? "" : undefined);
}

/** Changes one key of a stroke or mark; a value that can't be read starts over. */
export function setPart(kind: "stroke" | "mark", key: string, c: Change): (old: string) => string | null {
  return (old) => {
    const value = applyChange(c, partOf(kind, old, key) ?? "");
    return kind === "stroke"
      ? strokeSet(parseStroke(old) ?? { none: false, entries: [] }, key, value)
      : markSet(parseMark(old) ?? [], key, value);
  };
}
