// The editor's state: the source text (the document), the scene parsed from
// it, the latest compile (SVG + probe geometry), selection, tools and drags.
//
// Ids are UTF-8 byte offsets of calls in the *current* source. Whenever the
// source changes, everything holding ids (probes, selection) is remapped
// through the change's patches so it stays valid until the next compile.

import type { TypstCompiler, CompilerStatus, Diagnostic, ExportFormat } from "./compiler";
import type { FileHandle } from "./files";
import { OffsetIndex } from "./offsets";
import { formatStep, parseStep } from "./pixels";
import { isVec, probeBounds, transformPoint, untransformDelta, type Probe, type Vec3 } from "./probe";
import {
  allCalls,
  applyEdit,
  baseName,
  mapOffset,
  parseScene,
  STATE_CALLS,
  utf8Length,
  type Call,
  type Edit,
  type Layer,
  type Patch,
  type Range,
} from "./scene";

export type Tool = "select" | "line" | "arrow" | "rect" | "circle" | "polygon" | "star" | "arc" | "text" | "point" | "join" | "curve" | "brace" | "angle";

/** What the code pane exposes to the editor. */
export interface CodeHandle {
  /**
   * Applies a sequence of patch lists — each in byte offsets of the text the
   * previous one produced — as one undoable change.
   */
  applyPatches(steps: { patches: Patch[]; index: OffsetIndex }[]): void;
  /** Replaces the whole document; `resetHistory` for opening a new file. */
  replaceAll(text: string, resetHistory: boolean): void;
  undo(): void;
  redo(): void;
  reveal(range: Range): void;
  /** Re-measures layout, e.g. after the pane was hidden. */
  refresh(): void;
  /** Selects a range in the code and scrolls to it. */
  select(range: Range): void;
}

/** Where a canvas sits on the page: canvas (0, 0) and points per unit. */
export interface Frame {
  origin: { x: number; y: number };
  length: number;
}

export interface Instance {
  call: number;
  index: number;
}

export interface Placed {
  page: [number, number];
  frame: Frame;
  transform?: number[][];
}

export interface ChainState {
  source: string;
  created: number[];
  map: (id: number) => number;
}

interface Request {
  source: string;
  /** For drag previews: the move applied on top of the current source. */
  draft?: Draft;
}

/** View settings shared by every open document, like the toolbar they live in. */
export class Prefs {
  tool = $state<Tool>("select");
  snap = $state(true);
  /** The grid step for canvases whose file doesn't give one. */
  gridStep = $state(0.2);
  /** Keep each canvas's grid step in its file, as a comment above it. */
  gridInFile = $state(true);
  showGrid = $state(true);
  /** Show every shared point's marker, not just the selection's. */
  showPoints = $state(false);
  /** View the page as an endless sheet: no page edge, grid everywhere. */
  infinite = $state(false);
  /** Rulers in canvas units along the canvas's top and left edges. */
  showRulers = $state(true);
}

interface Draft {
  source: string;
  patches: Patch[];
  dx: number;
  dy: number;
}

/**
 * What this app last put on the clipboard, shared by every tab: pasting it
 * back into the editor it came from steps each paste along like ⌘D, unless
 * it was cut (then the first paste lands in place).
 */
let lastCopy: { text: string; from: Editor; pastes: number } | undefined;

export class Editor {
  source = $state("");
  /** A previewed, uncommitted edit (a drag in progress). */
  draft = $state<Draft>();
  /** A move committed before its compile arrived; keeps shapes in place. */
  pendingMove = $state<{ dx: number; dy: number }>();
  /** The drag delta the current probes were compiled with. */
  compiledDelta = $state({ dx: 0, dy: 0 });

  scene = $derived(parseScene(this.source));
  index = $derived(new OffsetIndex(this.source));
  calls = $derived(allCalls(this.scene));
  callById = $derived(new Map(this.calls.map((c) => [c.id, c])));
  pointById = $derived(new Map(this.scene.points.map((p) => [p.id, p])));
  /** Which calls use each shared point. */
  pointUsers = $derived.by(() => {
    const users = new Map<number, number[]>();
    for (const call of this.calls) {
      for (const arg of call.args) {
        if (arg.point !== null) users.set(arg.point, [...(users.get(arg.point) ?? []), call.id]);
      }
    }
    return users;
  });
  canvasOfCall = $derived(new Map(this.scene.canvases.flatMap((cv) => cv.calls.map((c) => [c.id, cv.id] as const))));

  selection = $state<number[]>([]);
  selected = $derived(this.selection.filter((id) => this.callById.has(id)));
  /** Shared points picked on the canvas (their definitions' offsets). */
  pointSelection = $state<number[]>([]);
  selectedPoints = $derived(this.pointSelection.filter((id) => this.pointById.has(id)));
  hovered = $state<number>();
  /** A group the user has entered (double-click) to select its children. */
  scope = $state<number>();
  /** Groups folded closed in the outline. */
  collapsed = $state(new Set<number>());
  get tool() { return this.prefs.tool; }
  set tool(v) { this.prefs.tool = v; }
  get snap() { return this.prefs.snap; }
  set snap(v) { this.prefs.snap = v; }
  /** The active canvas's grid step: from its file's comment, else the default. */
  gridStep = $derived.by(() => {
    const grid = this.prefs.gridInFile ? this.scene.canvases.find((c) => c.id === this.activeCanvas)?.grid : null;
    return (grid && parseStep(grid)) || this.prefs.gridStep;
  });
  get showGrid() { return this.prefs.showGrid; }
  set showGrid(v) { this.prefs.showGrid = v; }
  get showPoints() { return this.prefs.showPoints; }
  set showPoints(v) { this.prefs.showPoints = v; }
  get infinite() { return this.prefs.infinite; }
  set infinite(v) { this.prefs.infinite = v; }
  get showRulers() { return this.prefs.showRulers; }
  set showRulers(v) { this.prefs.showRulers = v; }
  hoveredPoint = $state<number>();
  /** A point whose name is being edited in the outline (e.g. just placed). */
  renamingPoint = $state<number>();
  /** Where the current hover came from: the canvas shows panel hovers more strongly. */
  hoverSource = $state<"canvas" | "panel">("canvas");
  /** One repetition of a call in a loop: the call id and which probe of it. */
  hoveredInstance = $state<Instance>();
  focusedInstance = $state<Instance>();

  /** Screen pixels per page point, and where the page's corner sits. */
  zoom = $state(1);
  pan = $state<[number, number]>([0, 0]);
  /** Registered by the canvas. */
  viewport?: { fit(): void; zoomBy(factor: number): void };
  /** Registered by the inspector: focus its primary text field. */
  focusInspector?: () => void;
  /** Registered by the app: show the code panel if it's collapsed. */
  openCode?: () => void;

  svg = $state<string>();
  probes = $state<Probe[]>([]);
  pageHeights = $state<number[]>([]);
  status = $state<CompilerStatus>({ kind: "loading" });
  diagnostics = $derived<Diagnostic[]>(this.status.kind === "done" ? this.status.diagnostics : []);
  hasErrors = $derived(this.diagnostics.some((d) => d.error));
  /** A short-lived message, e.g. an edit that couldn't be applied. */
  notice = $state<string>();

  fileName = $state("untitled.typ");
  /** Whether saving writes straight back to a file on disk (vs. asking where). */
  fileLinked = $state(false);
  /** The file on disk saving writes to, when the browser gave us one. */
  handle = $state.raw<FileHandle>();
  /** The file's line ending. The source always uses `\n`; saving restores this. */
  lineEnding = $state<"\n" | "\r\n">("\n");
  /** Bumped whenever a document is loaded, so views can reset (e.g. refit). */
  loads = $state(0);
  /** The load the canvas last fitted to the window, so it fits each one once. */
  fittedLoad?: number;
  savedSource = $state("");
  dirty = $derived(this.source !== this.savedSource);

  /** Every probe of each call, in drawing order (several for calls in loops). */
  probesById = $derived.by(() => {
    const map = new Map<number, Probe[]>();
    for (const p of this.probes) map.set(p.id, [...(map.get(p.id) ?? []), p]);
    return map;
  });

  /**
   * Where a shared point is on the page, and the frame its literal is written
   * in: from the anchor it defines if there is one, else from a shape using it.
   */
  placePoint(id: number): Placed | undefined {
    const p = this.pointById.get(id);
    if (!p) return undefined;
    const frameOf = (probe: Probe): Frame => ({ origin: probe.origin, length: probe.length });
    const anchor = this.probes.find((probe) => {
      const callee = this.callById.get(probe.id)?.callee ?? "";
      return probe.name !== null && p.anchors.includes(probe.name) && callee.slice(callee.lastIndexOf(".") + 1) === "anchor";
    });
    const value = anchor?.anchors["default"];
    if (anchor && isVec(value)) {
      return { page: this.toPage(frameOf(anchor), value), frame: frameOf(anchor), transform: anchor.transform };
    }
    for (const user of this.pointUsers.get(id) ?? []) {
      const probe = this.probesById.get(user)?.[0];
      if (probe) {
        return { page: this.toPage(frameOf(probe), transformPoint(probe.transform, [p.x, p.y])), frame: frameOf(probe), transform: probe.transform };
      }
    }
    const frame = this.frameFor(this.scene.canvases[0]?.id);
    return { page: this.toPage(frame, [p.x, p.y]), frame };
  }

  /** Canvas placement on the page, from the probes. */
  frames = $derived.by(() => {
    const frames = new Map<number, Frame>();
    for (const p of this.probes) {
      const canvas = this.canvasOfCall.get(p.id);
      if (canvas !== undefined && !frames.has(canvas)) {
        frames.set(canvas, { origin: { x: p.origin.x, y: p.origin.y }, length: p.length });
      }
    }
    return frames;
  });

  code?: CodeHandle;
  #compiler: TypstCompiler;
  #requests = new Map<number, Request>();
  #noticeTimer?: ReturnType<typeof setTimeout>;

  constructor(
    source: string,
    compiler: TypstCompiler,
    readonly prefs: Prefs,
  ) {
    source = normalizeNewlines(source);
    this.source = source;
    this.savedSource = source;
    this.#compiler = compiler;
  }

  // --- Compiling -----------------------------------------------------------

  compile() {
    const draft = this.draft;
    const source = draft?.source ?? this.source;
    if (this.status.kind === "done") this.status = { kind: "compiling" };
    const id = this.#compiler.compile(source, (status, id) => this.#onCompiled(status, id));
    this.#requests.set(id, { source: this.source, draft });
  }

  /** The document as a file, compiled as written. */
  export(format: ExportFormat, pixelPerPt?: number): Promise<Uint8Array> {
    return this.#compiler.export(this.source, format, pixelPerPt);
  }

  #onCompiled(status: CompilerStatus, id: number) {
    this.status = status;
    if (status.kind !== "done") return;
    const request = this.#requests.get(id);
    for (const key of this.#requests.keys()) if (key <= id) this.#requests.delete(key);
    // A result for an older source is superseded by a compile already queued.
    // Older drafts of the current source are fine: `moveOffset` makes up the
    // difference, which keeps a drag live while compiles lag behind it.
    if (!status.svg || !request || request.source !== this.source) return;

    this.svg = status.svg;
    this.pageHeights = status.pageHeights ?? [];
    const probes = status.probes ?? [];
    if (request.draft) {
      // Ids are offsets in the draft text; map them back to the source.
      const patches = request.draft.patches;
      this.probes = probes.map((p) => ({ ...p, id: unmapOffset(patches, p.id) }));
      this.compiledDelta = { dx: request.draft.dx, dy: request.draft.dy };
    } else {
      this.probes = probes;
      this.compiledDelta = { dx: 0, dy: 0 };
      this.pendingMove = undefined;
    }
  }

  // --- Source changes ------------------------------------------------------

  /** Called by the code pane for every document change, with byte patches. */
  sourceChanged(next: string, patches: Patch[]) {
    const map = (offset: number) => mapOffset(patches, offset, utf8Length);
    this.probes = this.probes.map((p) => ({ ...p, id: map(p.id) }));
    this.selection = this.selection.map(map);
    this.pointSelection = this.pointSelection.map(map);
    if (this.scope !== undefined) this.scope = map(this.scope);
    if (this.collapsed.size > 0) this.collapsed = new Set([...this.collapsed].map(map));
    this.hovered = undefined;
    this.hoveredPoint = undefined;
    this.hoveredInstance = undefined;
    this.focusedInstance = undefined;
    this.source = next;
  }

  /** Applies an edit as one undoable change. Returns false if it failed. */
  edit(edit: Edit): boolean {
    return this.chain([edit]);
  }

  /**
   * Applies dependent edits as one undoable change. Later steps can be
   * functions of the chain so far: `map` takes an id in the original source
   * to the current text, `created` is what the previous step created.
   */
  chain(steps: (Edit | ((state: ChainState) => Edit | undefined))[]): boolean {
    let source = this.source;
    let index = this.index;
    let created: number[] = [];
    const applied: { patches: Patch[]; index: OffsetIndex; after: string }[] = [];
    const map = (id: number) => applied.reduce((o, s) => mapOffset(s.patches, o, utf8Length), id);
    try {
      for (const step of steps) {
        const edit = typeof step === "function" ? step({ source, created, map }) : step;
        if (!edit) continue;
        const result = applyEdit(source, edit);
        // Created ids from earlier steps move with this step's patches.
        created = result.created.length > 0 ? result.created : created.map((id) => mapOffset(result.patches, id, utf8Length));
        applied.push({ patches: result.patches, index, after: result.source });
        source = result.source;
        index = new OffsetIndex(source);
      }
    } catch (err) {
      this.flash(err instanceof Error ? err.message : String(err));
      return false;
    }
    if (source === this.source) return true;
    if (this.code) {
      this.code.applyPatches(applied);
    } else {
      for (const step of applied) this.sourceChanged(step.after, step.patches);
    }
    if (created.length > 0) this.selection = created;
    return true;
  }

  /** Replaces the document, e.g. when opening a file. */
  load(source: string, fileName: string) {
    this.lineEnding = /\r\n/.test(source) && !/(^|[^\r])\n/.test(source) ? "\r\n" : "\n";
    source = normalizeNewlines(source);
    this.draft = undefined;
    this.pendingMove = undefined;
    this.selection = [];
    this.pointSelection = [];
    this.scope = undefined;
    this.probes = [];
    this.svg = undefined;
    this.fileName = fileName;
    this.savedSource = source;
    this.loads++;
    if (this.code) this.code.replaceAll(source, true);
    else this.source = source;
  }

  /** How a point is shown: its anchor name, else how code refers to it. */
  pointLabel(id: number): string {
    const p = this.pointById.get(id);
    return p ? (p.anchors[0] ?? p.path) : "";
  }

  /** Deletes the selected shapes and points (see `remove`). */
  deleteSelection() {
    this.remove(this.selected, this.selectedPoints);
  }

  /**
   * Deletes shapes and points as one undoable change, and clears the
   * selection. Points' remaining uses keep their position as coordinates;
   * says how many.
   */
  remove(shapes: number[], points: number[]) {
    // An `anchor(..)` defining a deleted point goes with the point.
    const definesPoint = (id: number) => {
      const call = this.callById.get(id);
      return call !== undefined && baseName(call.callee) === "anchor" && call.args.some((a) => a.point !== null && points.includes(a.point));
    };
    const calls = shapes.filter((id) => !definesPoint(id));
    if (points.length === 0 && calls.length === 0) return;
    const deleted = new Set(calls);
    const kept = points.flatMap((id) =>
      (this.pointUsers.get(id) ?? []).filter((c) => !deleted.has(c) && baseName(this.callById.get(c)?.callee ?? "") !== "anchor"),
    ).length;
    const steps: ((state: ChainState) => Edit | undefined)[] = [];
    if (points.length > 0) steps.push(() => ({ kind: "delete-points", points }));
    // Shapes go after: their ids move with the point deletion's patches.
    if (calls.length > 0) steps.push(({ map }) => ({ kind: "delete", calls: calls.map(map) }));
    if (!this.chain(steps)) return;
    this.selection = [];
    this.pointSelection = [];
    if (points.length > 0) {
      const what = points.length === 1 ? `Deleted ${this.pointLabel(points[0])}` : `Deleted ${points.length} points`;
      this.flash(kept > 0 ? `${what} · ${kept} use${kept === 1 ? "" : "s"} kept as coordinates` : what);
    }
  }

  // --- Align and distribute ---------------------------------------------------

  /** A shape's bounds on the page, children included. */
  boundsOf(id: number): { x0: number; y0: number; x1: number; y1: number } | undefined {
    const family = this.family(id);
    let box: { x0: number; y0: number; x1: number; y1: number } | undefined;
    for (const probe of this.probes.filter((p) => family.has(p.id))) {
      const b = probeBounds(probe);
      if (!b) continue;
      const frame = { origin: probe.origin, length: probe.length };
      const [ax, ay] = this.toPage(frame, [b.x0, b.y1]);
      const [bx, by] = this.toPage(frame, [b.x1, b.y0]);
      box = box ? { x0: Math.min(box.x0, ax), y0: Math.min(box.y0, ay), x1: Math.max(box.x1, bx), y1: Math.max(box.y1, by) } : { x0: ax, y0: ay, x1: bx, y1: by };
    }
    return box;
  }

  /**
   * Lines the selected shapes up by their bounds (`left` … `bottom`), or
   * spaces three or more evenly across (`across`) or down (`down`), as one
   * edit that moves each shape.
   */
  alignSelection(how: "left" | "center" | "right" | "top" | "middle" | "bottom" | "across" | "down") {
    const items = this.selected.flatMap((id) => {
      const b = this.boundsOf(id);
      const probe = this.probesById.get(id)?.[0];
      return b && probe ? [{ id, b, probe }] : [];
    });
    if (items.length < 2) return;
    const x0 = Math.min(...items.map((i) => i.b.x0));
    const x1 = Math.max(...items.map((i) => i.b.x1));
    const y0 = Math.min(...items.map((i) => i.b.y0));
    const y1 = Math.max(...items.map((i) => i.b.y1));
    // Page-space shift for each shape.
    const shift = new Map<number, [number, number]>();
    for (const { id, b } of items) {
      if (how === "left") shift.set(id, [x0 - b.x0, 0]);
      if (how === "right") shift.set(id, [x1 - b.x1, 0]);
      if (how === "center") shift.set(id, [(x0 + x1) / 2 - (b.x0 + b.x1) / 2, 0]);
      if (how === "top") shift.set(id, [0, y0 - b.y0]);
      if (how === "bottom") shift.set(id, [0, y1 - b.y1]);
      if (how === "middle") shift.set(id, [0, (y0 + y1) / 2 - (b.y0 + b.y1) / 2]);
    }
    if (how === "across" || how === "down") {
      if (items.length < 3) return;
      const horizontal = how === "across";
      const start = (i: (typeof items)[number]) => (horizontal ? i.b.x0 : i.b.y0);
      const size = (i: (typeof items)[number]) => (horizontal ? i.b.x1 - i.b.x0 : i.b.y1 - i.b.y0);
      const sorted = [...items].sort((a, b) => start(a) + size(a) / 2 - (start(b) + size(b) / 2));
      const span = (horizontal ? x1 - x0 : y1 - y0) - sorted.reduce((n, i) => n + size(i), 0);
      const gap = span / (sorted.length - 1);
      let at = horizontal ? x0 : y0;
      for (const i of sorted) {
        shift.set(i.id, horizontal ? [at - start(i), 0] : [0, at - start(i)]);
        at += size(i) + gap;
      }
    }
    const edits: Edit[] = items.flatMap(({ id, probe }) => {
      const [px, py] = shift.get(id) ?? [0, 0];
      if (Math.abs(px) < 1e-9 && Math.abs(py) < 1e-9) return [];
      const [dx, dy] = untransformDelta(probe.transform, [px / probe.length, -py / probe.length]);
      return [{ kind: "move", calls: [id], dx: Math.round(dx * 1e4) / 1e4, dy: Math.round(dy * 1e4) / 1e4 }];
    });
    if (edits.length) this.edit({ kind: "batch", edits });
  }

  // --- Clipboard ------------------------------------------------------------

  /** The selected shapes' source in code order, each statement dedented to its own line: what ⌘C copies. */
  selectionText(): string | undefined {
    const calls = this.selected
      .map((id) => this.callById.get(id))
      .filter((c) => c !== undefined)
      .sort((a, b) => a.id - b.id);
    if (calls.length === 0) return undefined;
    return calls
      .map((c) => {
        const before = this.index.slice(0, c.range.start);
        const indent = /[ \t]*$/.exec(before.slice(before.lastIndexOf("\n") + 1))?.[0] ?? "";
        const text = this.index.slice(c.range.start, c.range.end);
        return indent ? text.replaceAll(`\n${indent}`, "\n") : text;
      })
      .join("\n");
  }

  /** Copies the selection's source; with `cut`, deletes it too. Returns the text, if anything was selected. */
  copySelection(cut = false): string | undefined {
    const text = this.selectionText();
    if (text === undefined) return undefined;
    lastCopy = { text, from: this, pastes: cut ? -1 : 0 };
    if (cut) this.deleteSelection();
    return text;
  }

  /** Pastes CeTZ source at the end of the active canvas and selects it. */
  paste(text: string) {
    const again = lastCopy && lastCopy.text === text && lastCopy.from === this;
    const n = again ? ++lastCopy!.pastes : 0;
    this.edit({ kind: "paste", canvas: this.activeCanvas ?? null, text, dx: 0.5 * n, dy: -0.5 * n });
  }

  /** Wraps the selected shapes in a new group, which becomes the selection. */
  groupSelection() {
    if (this.selected.length) this.edit({ kind: "group", calls: this.selected });
  }

  /** Replaces each selected group with its children, which become the selection. */
  ungroupSelection() {
    // A rotated group's shapes stay in its scope, so they stay turned.
    const groups = this.selected.map((id) => this.wrappedShape(id)?.id ?? id).filter((id) => this.isGroup(id));
    if (groups.length) this.edit({ kind: "ungroup", calls: groups });
  }

  /** Moves the selection in front of or behind other shapes, by moving its code. */
  arrangeSelection(to: Layer) {
    if (this.selected.length) this.edit({ kind: "arrange", calls: this.selected, to });
  }

  /**
   * Sets the grid step: in the active canvas's comment when steps are kept
   * in the file, else as the default. Returns whether it went to the file.
   */
  setGridStep(step: number): boolean {
    const canvas = this.activeCanvas;
    if (this.prefs.gridInFile && canvas !== undefined) {
      this.edit({ kind: "set-grid", canvas, step: formatStep(step) });
      return true;
    }
    this.prefs.gridStep = step;
    return false;
  }

  /** Moves every `anchor(..)` up to the top of its block, as far as it can go. */
  gatherAnchors() {
    if (!this.edit({ kind: "gather-anchors" })) return;
    this.selection = [];
    this.pointSelection = [];
    this.flash("Gathered the anchors at the top");
  }

  isGroup(id: number): boolean {
    return baseName((this.wrappedShape(id) ?? this.callById.get(id))?.callee ?? "") === "group";
  }

  /**
   * A `scope({ rotate(..); scale(..); shape })` the transform handles wrap
   * a shape in (either transform may be missing, not both), given the scope
   * or its shape.
   */
  wrapper(id: number): { scope: Call; transforms: Call[]; shape: Call } | undefined {
    const of = (scopeId: number) => {
      const scope = this.callById.get(scopeId);
      if (!scope || baseName(scope.callee) !== "scope" || scope.in_loop) return undefined;
      const children = this.calls.filter((c) => c.parent === scopeId).sort((a, b) => a.id - b.id);
      const shape = children.pop();
      const kinds = children.map((c) => baseName(c.callee)).join(",");
      if (!shape || !["rotate", "scale", "rotate,scale"].includes(kinds) || STATE_CALLS.has(baseName(shape.callee))) return undefined;
      return { scope, transforms: children, shape };
    };
    const call = this.callById.get(id);
    const parent = call?.parent;
    return of(id) ?? (parent !== null && parent !== undefined ? of(parent) : undefined);
  }

  /** The shape of a wrapper scope (see `wrapper`), so the scope can be edited as that shape. */
  wrappedShape(id: number): Call | undefined {
    const w = this.wrapper(id);
    return w && w.scope.id === id ? w.shape : undefined;
  }

  flash(message: string) {
    this.notice = message;
    clearTimeout(this.#noticeTimer);
    this.#noticeTimer = setTimeout(() => (this.notice = undefined), 4000);
  }

  // --- Dragging ------------------------------------------------------------

  /** Previews moving the selection by a canvas-unit delta. */
  dragMove(dx: number, dy: number, detach = false) {
    this.previewEdit({ kind: "move", calls: this.selected, dx, dy, detach }, dx, dy);
  }

  /**
   * Previews any edit (used for handle drags); `dx`/`dy` shift the overlay.
   * False if the edit can't apply.
   */
  previewEdit(edit: Edit, dx = 0, dy = 0): boolean {
    if (dx === 0 && dy === 0 && edit.kind === "move") {
      this.draft = undefined;
      return true;
    }
    try {
      const result = applyEdit(this.source, edit);
      this.draft = { source: result.source, patches: result.patches, dx, dy };
      return true;
    } catch {
      // Keep the last valid preview.
      return false;
    }
  }

  /** Ends a drag, committing `edit` (or cancelling when undefined). */
  endDrag(edit?: Edit) {
    const draft = this.draft;
    this.draft = undefined;
    if (!edit || !draft) return;
    // Any drag previewed with a shift (a move, a shape's only handle) keeps it until the result compiles.
    if (edit.kind === "move" || draft.dx || draft.dy) this.pendingMove = { dx: draft.dx, dy: draft.dy };
    if (!this.edit(edit)) this.pendingMove = undefined;
  }

  /** How far to shift the selection's probed geometry so it shows where it will be. */
  moveOffset = $derived.by(() => {
    const target = this.draft ?? this.pendingMove;
    const dx = (target?.dx ?? 0) - this.compiledDelta.dx;
    const dy = (target?.dy ?? 0) - this.compiledDelta.dy;
    return { dx, dy };
  });

  // --- Geometry helpers ----------------------------------------------------

  /** The canvas new shapes go into: the selection's, else the first. */
  activeCanvas = $derived.by(() => {
    const fromSelection = this.selected.map((id) => this.canvasOfCall.get(id)).find((c) => c !== undefined);
    return fromSelection ?? this.scene.canvases[0]?.id;
  });

  frameFor(canvas: number | undefined): Frame {
    // Before anything is drawn there's no probe; assume CeTZ's 1cm default at the page corner.
    return (canvas !== undefined && this.frames.get(canvas)) || { origin: { x: 0, y: 0 }, length: 28.3465 };
  }

  toCanvas(frame: Frame, [x, y]: [number, number]): [number, number] {
    return [(x - frame.origin.x) / frame.length, (frame.origin.y - y) / frame.length];
  }

  toPage(frame: Frame, [x, y]: Vec3 | [number, number]): [number, number] {
    return [frame.origin.x + x * frame.length, frame.origin.y - y * frame.length];
  }

  snapValue(v: number): number {
    return this.snap ? Math.round(v / this.gridStep) * this.gridStep : v;
  }

  /** Calls the canvas lets you click: top level, or children of the entered group. */
  isSelectable(call: Call): boolean {
    return call.parent === (this.scope ?? null);
  }

  /** The selectable call a probe belongs to (itself or its nearest selectable ancestor). */
  selectableFor(id: number): number | undefined {
    let call = this.callById.get(id);
    while (call && !this.isSelectable(call)) {
      call = call.parent === null ? undefined : this.callById.get(call.parent);
    }
    return call?.id;
  }

  /** The call plus all calls nested inside it. */
  family(id: number): Set<number> {
    const call = this.callById.get(id);
    if (!call) return new Set();
    return new Set(this.calls.filter((c) => c.range.start >= call.range.start && c.range.end <= call.range.end).map((c) => c.id));
  }

  callText(call: Call): string {
    return this.index.slice(call.range.start, call.range.end);
  }
}

/** CodeMirror (and so the editor) works in `\n`-only text. */
export function normalizeNewlines(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

/** Maps an offset in patched text back to the original text. */
export function unmapOffset(patches: Patch[], offset: number): number {
  let shift = 0;
  for (const p of patches) {
    const start = p.start + shift;
    if (offset < start) break;
    const len = utf8Length(p.text);
    if (offset < start + len) return p.start;
    shift += len - (p.end - p.start);
  }
  return offset - shift;
}
