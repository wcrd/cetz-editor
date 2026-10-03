// Types for the Rust scene model (crates/scene) and typed wasm wrappers.

import { apply_edit, scene as sceneJson } from "./core";

export type Range = { start: number; end: number };

export type Value =
  | { type: "coord"; x: number; y: number; x_range: Range; y_range: Range }
  | { type: "number"; value: number }
  | { type: "str"; value: string }
  | { type: "content"; inner: Range }
  | { type: "expr" };

export interface Arg {
  key: string | null;
  range: Range;
  value_range: Range;
  text: string;
  value: Value;
  /** The shared point this argument uses (`A`, `pts.A`, `"A"`), if any. */
  point: number | null;
}

/** A literal coordinate defined once and used by reference. */
export interface Point {
  /** Byte offset of the coordinate literal. */
  id: number;
  /** How code refers to it: `A`, `pts.A`, `pts[0]`, or an anchor name. */
  path: string;
  x: number;
  y: number;
  range: Range;
  x_range: Range;
  y_range: Range;
  /** CeTZ anchor names bound to it. */
  anchors: string[];
}

export interface Call {
  /** Byte offset of the call; matches the probe id. */
  id: number;
  range: Range;
  callee: string;
  name: string | null;
  args: Arg[];
  args_close: number | null;
  parent: number | null;
  in_loop: boolean;
  conditional: boolean;
}

export interface Canvas {
  id: number;
  body: Range;
  calls: Call[];
}

export interface Scene {
  canvases: Canvas[];
  points: Point[];
}

export type Edit =
  | { kind: "move"; calls: number[]; dx: number; dy: number; detach?: boolean }
  | { kind: "set-point"; point: number; x: number; y: number }
  | { kind: "extract-point"; call: number; arg: number; name: string | null }
  | { kind: "set-coord"; call: number; arg: number; x: number; y: number }
  | { kind: "set-arg-text"; call: number; arg: number; text: string }
  | { kind: "set-named"; call: number; key: string; text: string | null }
  | { kind: "delete"; calls: number[] }
  | { kind: "insert"; canvas: number | null; text: string }
  | { kind: "connect"; call: number; arg: number; target: number; anchor: string }
  | { kind: "duplicate"; calls: number[]; dx: number; dy: number };

/** A change in byte offsets of the source it applies to. */
export interface Patch {
  start: number;
  end: number;
  text: string;
}

export interface EditResult {
  source: string;
  patches: Patch[];
  created: number[];
}

export function parseScene(source: string): Scene {
  return JSON.parse(sceneJson(source));
}

/** Throws with the core's message if the edit can't be applied. */
export function applyEdit(source: string, edit: Edit): EditResult {
  return JSON.parse(apply_edit(source, JSON.stringify(edit)));
}

/**
 * Maps a byte offset through patches (sorted, in old offsets). Offsets inside
 * replaced text map to the patch start; inserts at an offset push it right.
 */
export function mapOffset(patches: Patch[], offset: number, utf8Length: (s: string) => number): number {
  let shift = 0;
  for (const p of patches) {
    if (p.start > offset) break;
    if (p.end <= offset) {
      shift += utf8Length(p.text) - (p.end - p.start);
    } else {
      return p.start + shift;
    }
  }
  return offset + shift;
}

const encoder = new TextEncoder();
export const utf8Length = (s: string) => (/^[\x00-\x7f]*$/.test(s) ? s.length : encoder.encode(s).length);

export function allCalls(scene: Scene): Call[] {
  return scene.canvases.flatMap((c) => c.calls);
}

/** Drawing commands that only change state; they have no shape to select. */
export const STATE_CALLS = new Set([
  "set-style",
  "set-ctx",
  "translate",
  "rotate",
  "scale",
  "set-origin",
  "set-transform",
  "set-viewport",
  "anchor",
  "copy-anchors",
  "get-ctx",
  "register-mark",
  "on-layer",
]);

export function baseName(callee: string): string {
  return callee.slice(callee.lastIndexOf(".") + 1);
}
