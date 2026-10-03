// The editor's state: the source text (the document), the scene parsed from
// it, the latest compile (SVG + probe geometry), selection, tools and drags.
//
// Ids are UTF-8 byte offsets of calls in the *current* source. Whenever the
// source changes, everything holding ids (probes, selection) is remapped
// through the change's patches so it stays valid until the next compile.

import { TypstCompiler, type CompilerStatus, type Diagnostic } from "./compiler";
import { OffsetIndex } from "./offsets";
import type { Probe, Vec3 } from "./probe";
import {
  allCalls,
  applyEdit,
  mapOffset,
  parseScene,
  utf8Length,
  type Call,
  type Edit,
  type Patch,
  type Range,
} from "./scene";

export type Tool = "select" | "line" | "arrow" | "rect" | "circle" | "text";

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
}

/** Where a canvas sits on the page: canvas (0, 0) and points per unit. */
export interface Frame {
  origin: { x: number; y: number };
  length: number;
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

interface Draft {
  source: string;
  patches: Patch[];
  dx: number;
  dy: number;
}

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
  canvasOfCall = $derived(new Map(this.scene.canvases.flatMap((cv) => cv.calls.map((c) => [c.id, cv.id] as const))));

  selection = $state<number[]>([]);
  selected = $derived(this.selection.filter((id) => this.callById.has(id)));
  hovered = $state<number>();
  /** A group the user has entered (double-click) to select its children. */
  scope = $state<number>();
  tool = $state<Tool>("select");
  snap = $state(true);
  gridStep = $state(0.25);
  showGrid = $state(true);

  /** Screen pixels per page point, and where the page's corner sits. */
  zoom = $state(1);
  pan = $state<[number, number]>([0, 0]);
  /** Registered by the canvas. */
  viewport?: { fit(): void; zoomBy(factor: number): void };
  /** Registered by the inspector: focus its primary text field. */
  focusInspector?: () => void;

  svg = $state<string>();
  probes = $state<Probe[]>([]);
  pageHeights = $state<number[]>([]);
  status = $state<CompilerStatus>({ kind: "loading" });
  diagnostics = $derived<Diagnostic[]>(this.status.kind === "done" ? this.status.diagnostics : []);
  hasErrors = $derived(this.diagnostics.some((d) => d.error));
  /** A short-lived message, e.g. an edit that couldn't be applied. */
  notice = $state<string>();

  fileName = $state("untitled.typ");
  /** The file's line ending. The source always uses `\n`; saving restores this. */
  lineEnding = $state<"\n" | "\r\n">("\n");
  /** Bumped whenever a document is loaded, so views can reset (e.g. refit). */
  loads = $state(0);
  savedSource = $state("");
  dirty = $derived(this.source !== this.savedSource);

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

  constructor(source: string) {
    source = normalizeNewlines(source);
    this.source = source;
    this.savedSource = source;
    this.#compiler = new TypstCompiler((status, id) => this.#onCompiled(status, id));
  }

  dispose() {
    this.#compiler.dispose();
  }

  // --- Compiling -----------------------------------------------------------

  compile() {
    const draft = this.draft;
    const source = draft?.source ?? this.source;
    if (this.status.kind === "done") this.status = { kind: "compiling" };
    const id = this.#compiler.compile(source);
    this.#requests.set(id, { source: this.source, draft });
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
    if (this.scope !== undefined) this.scope = map(this.scope);
    this.hovered = undefined;
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
    this.scope = undefined;
    this.probes = [];
    this.svg = undefined;
    this.fileName = fileName;
    this.savedSource = source;
    this.loads++;
    if (this.code) this.code.replaceAll(source, true);
    else this.source = source;
  }

  flash(message: string) {
    this.notice = message;
    clearTimeout(this.#noticeTimer);
    this.#noticeTimer = setTimeout(() => (this.notice = undefined), 4000);
  }

  // --- Dragging ------------------------------------------------------------

  /** Previews moving the selection by a canvas-unit delta. */
  dragMove(dx: number, dy: number) {
    this.previewEdit({ kind: "move", calls: this.selected, dx, dy }, dx, dy);
  }

  /** Previews any edit (used for handle drags); `dx`/`dy` shift the overlay. */
  previewEdit(edit: Edit, dx = 0, dy = 0) {
    if (dx === 0 && dy === 0 && edit.kind === "move") {
      this.draft = undefined;
      return;
    }
    try {
      const result = applyEdit(this.source, edit);
      this.draft = { source: result.source, patches: result.patches, dx, dy };
    } catch {
      // Keep the last valid preview.
    }
  }

  /** Ends a drag, committing `edit` (or cancelling when undefined). */
  endDrag(edit?: Edit) {
    const draft = this.draft;
    this.draft = undefined;
    if (!edit || !draft) return;
    if (edit.kind === "move") this.pendingMove = { dx: draft.dx, dy: draft.dy };
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
