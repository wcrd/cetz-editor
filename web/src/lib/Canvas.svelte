<script lang="ts">
  // The drawing surface: the Typst render, with an interaction overlay built
  // from the probe geometry. Works in page points (the SVG's viewBox units);
  // `zoom` maps points to screen pixels.
  import { tick, untrack } from "svelte";
  import type { Editor, Frame, Tool } from "./editor.svelte";
  import { isVec, pathData, probeBounds, transformPoint, untransformDelta, type Probe, type Vec3 } from "./probe";
  import { baseName, STATE_CALLS, type Call, type Edit, type Range } from "./scene";
  import { num } from "./format";
  import { crisp, visibleStep } from "./pixels";
  import Rulers from "./Rulers.svelte";

  let { editor }: { editor: Editor } = $props();

  let viewport: HTMLDivElement;
  let width = $state(0);
  let height = $state(0);
  let spaceHeld = $state(false);
  /** The pointer over the canvas, in screen pixels. */
  let pointer = $state<Point>();

  type Point = [number, number];
  /**
   * Somewhere a drag can snap to: another shape's anchor (`target` +
   * `anchor`, written as `"name.anchor"`), a named point (`named`,
   * written as `ref`), or a line's literal vertex (`vertex`, named on use).
   */
  type Snap = { point: Point; target?: number; anchor?: string; named?: number; ref?: string; vertex?: { call: number; arg: number } };
  type Drag =
    | { kind: "pan"; start: Point; pan: Point }
    | { kind: "move"; start: Point; moved: boolean; delta: Point; detach: boolean; box?: Box }
    | { kind: "handle"; call: number; arg: number; probe: Probe; snap?: Snap; edit?: Edit }
    | {
        kind: "point";
        point: number;
        start: Point;
        moved: boolean;
        /** Frame and transform the point's literal is written in. */
        frame: Frame;
        transform?: number[][];
        /** Set when dragging one use of the point (a shape's handle): Alt detaches it. */
        use?: { call: number; arg: number; probe: Probe };
        /** Other selected points that move along with it. */
        group?: number[];
        detach: boolean;
        snap?: Snap;
        edit?: Edit;
      }
    | { kind: "marquee"; start: Point; end: Point; base: number[]; basePoints: number[] }
    | { kind: "rotate"; spin: Spin; from: number; angle: number; edit?: Edit }
    | { kind: "radius"; reach: Reach; r: number; edit?: Edit }
    | { kind: "reshape"; reshape: Reshape; edit?: Edit }
    | { kind: "sweep"; end: ArcEnd; sweep: number; angle?: number; edit?: Edit }
    | { kind: "grow"; grow: Grow; from: number; factor: number; edit?: Edit }
    | { kind: "create"; start: Point; end: Point; startSnap?: Snap; endSnap?: Snap; frame: Frame; transform?: number[][] };
  let drag = $state<Drag>();

  const pageSize = $derived.by(() => {
    const m = editor.svg && /viewBox="([\d.\-e]+) ([\d.\-e]+) ([\d.\-e]+) ([\d.\-e]+)"/.exec(editor.svg);
    return m ? { w: parseFloat(m[3]), h: parseFloat(m[4]) } : undefined;
  });

  // --- View ----------------------------------------------------------------

  function fit() {
    if (!pageSize || !width || !height) return;
    const z = Math.min((width - 64) / pageSize.w, (height - 64) / pageSize.h, 4);
    editor.zoom = Math.max(0.1, z);
    editor.pan = [(width - pageSize.w * editor.zoom) / 2, (height - pageSize.h * editor.zoom) / 2];
  }

  /** Zoom a document opens at: 150%, or less if its page wouldn't fit. ⌘0 fits. */
  const DEFAULT_ZOOM = 1.5;

  function resetView() {
    if (!pageSize || !width || !height) return;
    const fits = Math.min((width - 64) / pageSize.w, (height - 64) / pageSize.h);
    editor.zoom = Math.max(0.1, Math.min(DEFAULT_ZOOM, fits));
    editor.pan = [(width - pageSize.w * editor.zoom) / 2, (height - pageSize.h * editor.zoom) / 2];
  }

  function zoomAt(factor: number, [sx, sy]: Point) {
    const z = Math.min(32, Math.max(0.1, editor.zoom * factor));
    const k = z / editor.zoom;
    editor.pan = [sx - (sx - editor.pan[0]) * k, sy - (sy - editor.pan[1]) * k];
    editor.zoom = z;
  }

  // Set up each loaded document's view once; switching back to its tab keeps its view.
  $effect(() => {
    if (pageSize && width && height && editor.fittedLoad !== editor.loads) {
      editor.fittedLoad = editor.loads;
      resetView();
    }
  });
  $effect(() => {
    editor.viewport = { fit, zoomBy: (f: number) => zoomAt(f, [width / 2, height / 2]) };
    return () => (editor.viewport = undefined);
  });

  // Auto-sized pages grow (and shift their content) as shapes are added at the
  // edges. Keep the drawing still on screen by panning by however far the
  // canvas origin moved on the page — including mid-drag.
  let lastOrigin: { x: number; y: number; loads: number } | undefined;
  $effect(() => {
    const frame = editor.frames.values().next().value;
    if (!frame) return;
    const { x, y } = frame.origin;
    untrack(() => {
      if (lastOrigin && lastOrigin.loads === editor.loads) {
        const dx = x - lastOrigin.x;
        const dy = y - lastOrigin.y;
        if (dx !== 0 || dy !== 0) {
          editor.pan = [editor.pan[0] - dx * editor.zoom, editor.pan[1] - dy * editor.zoom];
          shiftDrag(dx, dy);
        }
      }
      lastOrigin = { x, y, loads: editor.loads };
    });
  });

  /** Keeps an in-progress drag's page-space points in step with a page shift. */
  function shiftDrag(dx: number, dy: number) {
    if (!drag) return;
    const shift = (p: Point): Point => [p[0] + dx, p[1] + dy];
    switch (drag.kind) {
      case "move":
        drag.start = shift(drag.start);
        break;
      case "marquee":
        drag.start = shift(drag.start);
        drag.end = shift(drag.end);
        break;
      case "create":
        drag.frame = { ...drag.frame, origin: { x: drag.frame.origin.x + dx, y: drag.frame.origin.y + dy } };
        break;
    }
  }

  function onwheel(e: WheelEvent) {
    e.preventDefault();
    const r = viewport.getBoundingClientRect();
    if (e.ctrlKey || e.metaKey) {
      zoomAt(Math.exp(-e.deltaY * 0.01), [e.clientX - r.left, e.clientY - r.top]);
    } else {
      editor.pan = [editor.pan[0] - e.deltaX, editor.pan[1] - e.deltaY];
    }
  }

  // --- Coordinates ---------------------------------------------------------

  function pagePoint(e: MouseEvent): Point {
    const r = viewport.getBoundingClientRect();
    return [(e.clientX - r.left - editor.pan[0]) / editor.zoom, (e.clientY - r.top - editor.pan[1]) / editor.zoom];
  }

  function frameOf(probe: Probe): Frame {
    return { origin: probe.origin, length: probe.length };
  }

  /** A call's own coordinates → page points, through its CeTZ transform. */
  function localToPage(probe: Probe, p: Point): Point {
    return editor.toPage(frameOf(probe), transformPoint(probe.transform, p));
  }

  /** Page points → a call's own coordinates. */
  function pageToLocal(frame: Frame, transform: number[][] | undefined, p: Point): Point {
    const [cx, cy] = editor.toCanvas(frame, p);
    if (!transform) return [cx, cy];
    const [tx, ty] = [transform[0][3], transform[1][3]];
    return untransformDelta(transform, [cx - tx, cy - ty]);
  }

  const probeOf = $derived(new Map(editor.probes.map((p) => [p.id, p])));

  // --- Shapes --------------------------------------------------------------

  const shapes = $derived.by(() => {
    const seen = new Map<number, number>();
    return editor.probes.map((probe) => {
      const index = seen.get(probe.id) ?? 0;
      seen.set(probe.id, index + 1);
      return { probe, index, d: probe.drawables.map((d) => pathData(probe, d)).join(" "), target: editor.selectableFor(probe.id) };
    });
  });

  /** The single loop repetition being pointed at in the outline. */
  const instance = $derived(editor.hoveredInstance ?? editor.focusedInstance);
  const instanceShape = $derived(instance && shapes.find((s) => s.probe.id === instance.call && s.index === instance.index));

  const selectedFamily = $derived(new Set(editor.selected.flatMap((id) => [...editor.family(id)])));
  const hoveredFamily = $derived.by(() => {
    const ids = editor.hovered === undefined ? [] : [editor.hovered];
    // Hovering a shared point highlights every shape that uses it.
    if (editor.hoveredPoint !== undefined) ids.push(...(editor.pointUsers.get(editor.hoveredPoint) ?? []));
    return new Set(ids.flatMap((id) => [...editor.family(id)]));
  });

  /** Page-space offset that shows a moving selection where it will land. */
  const moveShift = $derived.by((): Point => {
    const { dx, dy } = editor.moveOffset;
    if (dx === 0 && dy === 0) return [0, 0];
    const probe = probeOf.get(editor.selected[0]);
    const m = probe?.transform;
    const [wx, wy] = m ? [m[0][0] * dx + m[0][1] * dy, m[1][0] * dx + m[1][1] * dy] : [dx, dy];
    const length = probe?.length ?? 28.3465;
    return [wx * length, -wy * length];
  });

  /** Page-space bounds of the selection, for its outline. */
  const selectionBox = $derived.by(() => {
    let box: { x0: number; y0: number; x1: number; y1: number } | undefined;
    for (const probe of editor.probes) {
      if (!selectedFamily.has(probe.id)) continue;
      const b = probeBounds(probe);
      if (!b) continue;
      const [ax, ay] = editor.toPage(frameOf(probe), [b.x0, b.y1]);
      const [bx, by] = editor.toPage(frameOf(probe), [b.x1, b.y0]);
      box = box
        ? { x0: Math.min(box.x0, ax), y0: Math.min(box.y0, ay), x1: Math.max(box.x1, bx), y1: Math.max(box.y1, by) }
        : { x0: ax, y0: ay, x1: bx, y1: by };
    }
    return box;
  });

  // --- Handles and anchors -------------------------------------------------

  type Handle = { call: number; arg: number; point: Point; linked: boolean; shared?: number };

  // --- Shared points -------------------------------------------------------

  const placed = $derived(new Map(editor.scene.points.map((p) => [p.id, editor.placePoint(p.id)] as const)));

  const pointLabel = (id: number) => editor.pointLabel(id);

  /**
   * Point markers: all of them with "Points" on, while drawing, or while
   * dragging a handle (they're snap targets then); else the hovered and
   * selected ones. Handles cover the selected shape's own points. Picking
   * which end of a selected line to continue shows just that line's.
   */
  const markers = $derived.by(() => {
    const withHandles = new Set(handles.map((h) => h.shared));
    const drawing = editor.tool !== "select" && !continuable;
    const all = editor.showPoints || drawing || drag?.kind === "handle" || (drag?.kind === "point" && drag.detach);
    const some = new Set(editor.selectedPoints);
    if (editor.hoveredPoint !== undefined) some.add(editor.hoveredPoint);
    const ids = all ? editor.scene.points.map((p) => p.id) : [...some];
    const selected = new Set(editor.selectedPoints);
    return ids
      .filter((id) => !withHandles.has(id) && placed.get(id))
      .map((id) => ({ id, page: placed.get(id)!.page, selected: selected.has(id) }));
  });

  /** Something a press in select mode picks up: a selected shape's handle, or a shared point. */
  type Grab = { handle: Handle } | { point: number };

  /**
   * The nearest handle or shared point within reach of `p`, shown or not:
   * nearness beats drawing order, so a crowded spot picks what's closest.
   * Handles win ties, as they carry which use of a shared point you meant.
   */
  function grabAt(p: Point): Grab | undefined {
    const dist = (q: Point) => Math.hypot(q[0] - p[0], q[1] - p[1]);
    let best: Grab | undefined;
    let bestDist = 10 / editor.zoom;
    for (const h of handles) {
      if (dist(h.point) < bestDist) [best, bestDist] = [{ handle: h }, dist(h.point)];
    }
    for (const [id, where] of placed) {
      if (where && dist(where.page) < bestDist) [best, bestDist] = [{ point: id }, dist(where.page)];
    }
    return best;
  }

  /** What the pointer would grab, while hovering in select mode. */
  let nearGrab = $state<Grab>();
  const nearHandle = $derived(nearGrab && "handle" in nearGrab ? nearGrab.handle.arg : undefined);

  /** True while a drag would move shared points (so Alt would detach). */
  const sharing = $derived(
    (drag?.kind === "point" && drag.use !== undefined) ||
      (drag?.kind === "move" && drag.moved && editor.selected.some((id) => editor.callById.get(id)?.args.some((a) => a.point !== null))),
  );
  const detaching = $derived((drag?.kind === "point" || drag?.kind === "move") && drag.detach);

  /** Draggable points of a single selected call: its coordinate arguments. */
  const handles = $derived.by((): Handle[] => {
    if (editor.selected.length !== 1 || drag?.kind === "move") return [];
    // A rotated shape's scope gets the shape's handles, in its turned frame.
    const call = editor.wrappedShape(editor.selected[0]) ?? editor.callById.get(editor.selected[0]);
    const probe = call && probeOf.get(call.id);
    if (!call || !probe || call.in_loop) return [];
    return call.args.flatMap((_, i) => argHandle(call, probe, i) ?? []);
  });

  /** Where a positional coordinate argument is on the page, as a handle. */
  function argHandle(call: Call, probe: Probe, i: number): Handle | undefined {
    const arg = call.args[i];
    if (arg.key !== null) return undefined;
    const shared = arg.point !== null ? placed.get(arg.point) : undefined;
    if (arg.point !== null && shared) return { call: call.id, arg: i, point: shared.page, linked: true, shared: arg.point };
    if (arg.value.type === "coord") return { call: call.id, arg: i, point: localToPage(probe, [arg.value.x, arg.value.y]), linked: false };
    const point = arg.value.type === "str" ? anchorPoint(arg.value.value) : undefined;
    return point && { call: call.id, arg: i, point, linked: true };
  }

  /** Resolves `"name.anchor"` against the probes. */
  function anchorPoint(ref: string): Point | undefined {
    const dot = ref.lastIndexOf(".");
    const [name, anchor] = dot < 0 ? [ref, "default"] : [ref.slice(0, dot), ref.slice(dot + 1)];
    const probe = editor.probes.find((p) => p.name === name.split(".").pop());
    const value = probe?.anchors[anchor] ?? (anchor === "default" ? probe?.anchors["center"] : undefined);
    return probe && isVec(value) ? editor.toPage(frameOf(probe), value) : undefined;
  }

  // --- Rotation -------------------------------------------------------------

  /**
   * How the selected shape turns: by its own `angle:` (polygons, stars,
   * text), by the `rotate(..)` its scope or group starts with, or by
   * wrapping it in `scope({ rotate(..) .. })` the first time. `pivot` is on
   * the page, `angle` is the current one in degrees, and `sign` is -1 when
   * the frame is mirrored, so CeTZ's counter-clockwise is clockwise on screen.
   */
  type Spin = { pivot: Point; angle: number; sign: number; edit: (angle: number) => Edit | undefined };

  /** Degrees from an angle's source text (`30deg`, `0.5rad`), or undefined. */
  function degrees(text: string): number | undefined {
    const m = /^\s*(-?[\d.]+)\s*(deg|rad)\s*$/.exec(text);
    if (!m) return undefined;
    return m[2] === "deg" ? Number(m[1]) : (Number(m[1]) * 180) / Math.PI;
  }

  const OWN_ANGLE = new Set(["polygon", "n-star", "content"]);

  /** -1 when a transform mirrors (so angles turn the other way on the page), else 1. */
  function handedness(m: number[][] | undefined): number {
    return m && m[0][0] * m[1][1] - m[0][1] * m[1][0] < 0 ? -1 : 1;
  }

  const spin = $derived.by((): Spin | undefined => {
    if (editor.selected.length !== 1) return undefined;
    const call = editor.callById.get(editor.selected[0]);
    const probe = call && probeOf.get(call.id);
    const base = call && baseName(call.callee);
    if (!call || !probe || !base || call.in_loop || STATE_CALLS.has(base) || probe.drawables.length === 0) return undefined;
    const sign = handedness(probe.transform);
    const center = isVec(probe.anchors.center) ? editor.toPage(frameOf(probe), probe.anchors.center) : undefined;

    if (OWN_ANGLE.has(base)) {
      const arg = call.args.find((a) => a.key === "angle");
      const angle = arg ? degrees(arg.text) : 0;
      const at = call.args.findIndex((a) => a.key === null);
      const pivot = (at >= 0 ? argHandle(call, probe, at)?.point : undefined) ?? center;
      if (angle === undefined || !pivot) return undefined;
      return { pivot, angle, sign, edit: (a) => ({ kind: "set-named", call: call.id, key: "angle", text: a ? `${num(a)}deg` : null }) };
    }

    // A scope or group that starts with a rotation turns by it; so does a
    // shape in a wrapper scope with one (when you've entered it).
    const turner = (c: Call) => {
      const children = editor.calls.filter((k) => k.parent === c.id).sort((a, b) => a.id - b.id);
      const kind = baseName(c.callee);
      return (kind === "scope" || kind === "group") && children[0] && baseName(children[0].callee) === "rotate" ? children : undefined;
    };
    const wrap = editor.wrapper(call.id);
    const owner = turner(call) ? call : wrap && baseName(wrap.transforms[0].callee) === "rotate" ? wrap.scope : undefined;
    const ownerProbe = owner && probeOf.get(owner.id);
    if (owner && ownerProbe) {
      const first = turner(owner)![0];
      const at = first.args.findIndex((a) => a.key === null);
      const angle = at >= 0 ? degrees(first.args[at].text) : undefined;
      const origin = first.args.find((a) => a.key === "origin")?.value;
      if (angle === undefined || (origin && origin.type !== "coord")) return undefined;
      const pivot = localToPage(ownerProbe, origin?.type === "coord" ? [origin.x, origin.y] : [0, 0]);
      // Back at 0°, a scope that only turned one shape goes away.
      const unwrap = editor.wrappedShape(owner.id) !== undefined;
      return {
        pivot,
        angle,
        sign: handedness(ownerProbe.transform),
        edit: (a) => (a === 0 && unwrap ? { kind: "unrotate", call: owner.id } : { kind: "set-arg-text", call: first.id, arg: at, text: `${num(a)}deg` }),
      };
    }

    const b = probeBounds(probe);
    const pivot = center ?? (b && editor.toPage(frameOf(probe), [(b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2]));
    if (!pivot) return undefined;
    const [x, y] = pageToLocal(frameOf(probe), probe.transform, pivot);
    return { pivot, angle: 0, sign, edit: (a) => (a ? { kind: "rotate", call: call.id, angle: a, x, y } : undefined) };
  });

  // --- Scale --------------------------------------------------------------

  /**
   * How the selected shape scales: about `pivot` (page) from `factor`, with
   * the edit for a new factor. It goes in the shape's wrapper scope beside
   * any rotation, about the rotation's pivot so the two don't fight.
   *
   * Only groups, scopes and your own functions get it, since their geometry
   * is out of reach; CeTZ's shapes have their own handles for size. A shape
   * that's already scaled keeps it, so the scale can be changed or undone.
   */
  type Grow = { pivot: Point; factor: number; edit: (factor: number) => Edit };

  const grow = $derived.by((): Grow | undefined => {
    if (!spin) return undefined;
    const call = editor.callById.get(editor.selected[0]);
    const probe = call && probeOf.get(call.id);
    if (!call || !probe) return undefined;
    const wrap = editor.wrapper(call.id);
    const outer = wrap && probeOf.get(wrap.scope.id);
    const rotate = wrap?.transforms.find((t) => baseName(t.callee) === "rotate");
    const scale = wrap?.transforms.find((t) => baseName(t.callee) === "scale");
    const shape = editor.wrappedShape(call.id) ?? call;
    const opaque =
      editor.calls.some((c) => c.parent === shape.id) || editor.scene.variables.some((v) => v.kind === "function" && v.name === shape.callee);
    if (!opaque && !scale) return undefined;
    const originOf = (t: Call | undefined) => {
      const v = t?.args.find((a) => a.key === "origin")?.value;
      return v?.type === "coord" ? ([v.x, v.y] as Point) : undefined;
    };
    const edit = (x: number, y: number) => (factor: number): Edit => ({ kind: "scale", call: call.id, factor, x, y });
    if (wrap && outer && scale) {
      const amount = scale.args.find((a) => a.key === null)?.value;
      const o = originOf(scale);
      if (amount?.type !== "number" || !o || scale.args.some((a) => a.key !== null && a.key !== "origin")) return undefined;
      // The scale's origin is in the rotated frame; the rotation's pivot is where the two agree.
      const r = originOf(rotate);
      const angle = rotate ? degrees(rotate.args.find((a) => a.key === null)?.text ?? "") : 0;
      if (angle === undefined || (rotate && !r)) return undefined;
      const turned: Point = r ? [r[0] + Math.cos((angle * Math.PI) / 180) * (o[0] - r[0]) - Math.sin((angle * Math.PI) / 180) * (o[1] - r[1]), r[1] + Math.sin((angle * Math.PI) / 180) * (o[0] - r[0]) + Math.cos((angle * Math.PI) / 180) * (o[1] - r[1])] : o;
      return { pivot: localToPage(outer, turned), factor: amount.value, edit: edit(o[0], o[1]) };
    }
    const r = originOf(rotate);
    if (wrap && outer && r) return { pivot: localToPage(outer, r), factor: 1, edit: edit(r[0], r[1]) };
    const [x, y] = pageToLocal(frameOf(probe), probe.transform, spin.pivot);
    return { pivot: spin.pivot, factor: 1, edit: edit(x, y) };
  });

  /** The scale handle: a knob off the selection's bottom-right corner. */
  const growKnob = $derived.by((): { knob: Point; stem: Point } | undefined => {
    if (!grow || !selectionBox || editor.tool !== "select" || (drag && drag.kind !== "grow")) return undefined;
    const stem: Point = [selectionBox.x1, selectionBox.y1];
    const off = 14 / editor.zoom;
    return { stem, knob: [stem[0] + off, stem[1] + off] };
  });

  let nearGrow = $state(false);

  function overGrow(p: Point): boolean {
    return !!growKnob && Math.hypot(p[0] - growKnob.knob[0], p[1] - growKnob.knob[1]) * editor.zoom < 9;
  }

  /** The rotation handle: a knob above the selection. */
  const spinHandle = $derived.by((): { knob: Point; stem: Point } | undefined => {
    if (!spin || !selectionBox || editor.tool !== "select" || (drag && drag.kind !== "rotate")) return undefined;
    const x = (selectionBox.x0 + selectionBox.x1) / 2;
    return { stem: [x, selectionBox.y0], knob: [x, selectionBox.y0 - 22 / editor.zoom] };
  });

  /** The pointer is over the rotation handle. */
  let nearSpin = $state(false);

  function overSpin(p: Point): boolean {
    return !!spinHandle && Math.hypot(p[0] - spinHandle.knob[0], p[1] - spinHandle.knob[1]) * editor.zoom < 9;
  }

  // --- Rect corners and edges ------------------------------------------------

  /**
   * A rect's (or grid's) other two corners and its four edges, which move
   * parts of its two literal corners. `edit` takes where it's dragged, local.
   */
  type Reshape = { point: Point; edge: boolean; probe: Probe; edit: (p: Point) => Edit | undefined };

  const reshapes = $derived.by((): Reshape[] => {
    if (editor.selected.length !== 1 || editor.tool !== "select" || (drag && drag.kind !== "reshape")) return [];
    const call = editor.wrappedShape(editor.selected[0]) ?? editor.callById.get(editor.selected[0]);
    const probe = call && probeOf.get(call.id);
    if (!call || !probe || call.in_loop || !["rect", "grid"].includes(baseName(call.callee))) return [];
    const at = call.args.flatMap((arg, i) => (arg.key === null ? [i] : [])).slice(0, 2);
    if (at.length < 2 || at.some((i) => call.args[i].point !== null)) return [];
    const [a, b] = at.map((i) => call.args[i].value);
    if (a.type !== "coord" || b.type !== "coord") return [];
    // Only the corners that change are written.
    const set = (pa: Point, pb: Point): Edit | undefined => {
      const edits: Edit[] = [];
      if (pa[0] !== a.x || pa[1] !== a.y) edits.push({ kind: "set-coord", call: call.id, arg: at[0], x: pa[0], y: pa[1] });
      if (pb[0] !== b.x || pb[1] !== b.y) edits.push({ kind: "set-coord", call: call.id, arg: at[1], x: pb[0], y: pb[1] });
      return edits.length ? { kind: "batch", edits } : undefined;
    };
    const [mx, my] = [(a.x + b.x) / 2, (a.y + b.y) / 2];
    const items: [Point, boolean, (p: Point) => Edit | undefined][] = [
      [[a.x, b.y], false, (p) => set([p[0], a.y], [b.x, p[1]])],
      [[b.x, a.y], false, (p) => set([a.x, p[1]], [p[0], b.y])],
      [[a.x, my], true, (p) => set([p[0], a.y], [b.x, b.y])],
      [[b.x, my], true, (p) => set([a.x, a.y], [p[0], b.y])],
      [[mx, a.y], true, (p) => set([a.x, p[1]], [b.x, b.y])],
      [[mx, b.y], true, (p) => set([a.x, a.y], [b.x, p[1]])],
    ];
    return items.map(([q, edge, edit]) => ({ point: localToPage(probe, q), edge, probe, edit }));
  });

  function reshapeAt(p: Point): Reshape | undefined {
    return reshapes.find((r) => Math.hypot(p[0] - r.point[0], p[1] - r.point[1]) * editor.zoom < 7);
  }

  let nearReshape = $state(false);

  // --- Radius ---------------------------------------------------------------

  /** The anchors a shape's radius handle sits on (on its outline) and measures from. */
  const RADIUS_ANCHORS: Record<string, [edge: string, center: string]> = {
    circle: ["east", "center"],
    polygon: ["corner-0", "center"],
    // A star's corners go inner, outer, ...; its radius is the outer one.
    "n-star": ["corner-1", "center"],
    arc: ["arc-center", "origin"],
  };

  /**
   * A radius handle of the selected shape: where it is and the centre
   * (page), the radius it measures (local), and the `radius:` text for a new
   * value (`stretch`: ⌥, which makes a circle an ellipse along this axis).
   */
  type Reach = { call: number; probe: Probe; point: Point; center: Point; r: number; text: (r: number, stretch: boolean) => string; stretchy?: boolean };

  const reaches = $derived.by((): Reach[] => {
    if (editor.selected.length !== 1 || editor.tool !== "select" || (drag && drag.kind !== "radius")) return [];
    const call = editor.wrappedShape(editor.selected[0]) ?? editor.callById.get(editor.selected[0]);
    const probe = call && probeOf.get(call.id);
    const base = call && baseName(call.callee);
    const names = base && RADIUS_ANCHORS[base];
    if (!call || !probe || !names || call.in_loop) return [];
    const at = (name: string) => {
      const v = probe.anchors[name];
      return isVec(v) ? editor.toPage(frameOf(probe), v) : undefined;
    };
    const center = at(names[1]);
    if (!center) return [];
    const reach = (name: string, text: Reach["text"]): Reach[] => {
      const point = at(name);
      if (!point) return [];
      const [a, b] = [center, point].map((q) => pageToLocal(frameOf(probe), probe.transform, q));
      return [{ call: call.id, probe, point, center, r: Math.hypot(b[0] - a[0], b[1] - a[1]), text }];
    };
    const arg = call.args.find((a) => a.key === "radius");
    // An ellipse, `radius: (x, y)`: one square per axis.
    if (base === "circle" && arg?.value.type === "coord") {
      const { x: rx, y: ry } = arg.value;
      return [...reach("east", (r) => `(${num(r)}, ${num(ry)})`), ...reach("north", (r) => `(${num(rx)}, ${num(r)})`)];
    }
    if (arg && arg.value.type !== "number") return [];
    const round = reach(names[0], (r, stretch) => {
      if (!stretch || base !== "circle") return num(r);
      const [a, b] = [center, at("north")!].map((q) => pageToLocal(frameOf(probe), probe.transform, q));
      return `(${num(r)}, ${num(Math.hypot(b[0] - a[0], b[1] - a[1]))})`;
    });
    return round.map((r) => ({ ...r, stretchy: base === "circle" }));
  });

  let nearReach = $state(false);

  // --- Arc ends -------------------------------------------------------------

  /**
   * A knob on one end of the selected arc: dragging it turns that end
   * around the centre and keeps the other where it is, writing whichever of
   * `start`, `stop` and `delta` the arc uses. `start`/`stop` are degrees.
   */
  type ArcEnd = { call: number; probe: Probe; end: "start" | "stop"; point: Point; center: Point; start: number; stop: number; keys: string[] };

  const arcEnds = $derived.by((): ArcEnd[] => {
    if (editor.selected.length !== 1 || editor.tool !== "select" || (drag && drag.kind !== "sweep")) return [];
    const call = editor.wrappedShape(editor.selected[0]) ?? editor.callById.get(editor.selected[0]);
    const probe = call && probeOf.get(call.id);
    if (!call || !probe || call.in_loop || baseName(call.callee) !== "arc") return [];
    const value = (key: string) => {
      const arg = call.args.find((a) => a.key === key);
      return arg ? degrees(arg.text) : undefined;
    };
    const keys = ["start", "stop", "delta"].filter((k) => call.args.some((a) => a.key === k));
    if (keys.length !== 2 || keys.some((k) => value(k) === undefined)) return [];
    const [a, b, d] = [value("start"), value("stop"), value("delta")];
    const start = a ?? b! - d!;
    const stop = b ?? a! + d!;
    const page = (name: string) => {
      const v = probe.anchors[name];
      return isVec(v) ? editor.toPage(frameOf(probe), v) : undefined;
    };
    const center = page("origin");
    if (!center) return [];
    // Off its centre, an arc is placed by its start, where its position handle sits.
    const anchor = call.args.find((a) => a.key === "anchor")?.text.trim();
    const ends: ArcEnd["end"][] = anchor === '"origin"' ? ["start", "stop"] : ["stop"];
    return ends.flatMap((end) => {
      const point = page(end === "start" ? "arc-start" : "arc-end");
      return point ? [{ call: call.id, probe, end, point, center, start, stop, keys }] : [];
    });
  });

  function arcEndAt(p: Point): ArcEnd | undefined {
    return arcEnds.find((k) => Math.hypot(p[0] - k.point[0], p[1] - k.point[1]) * editor.zoom < 8);
  }

  let nearArcEnd = $state(false);

  /** Moves an arc's end to the pointer's angle; `sweep` keeps the turn continuous past ±180°. */
  function sweepTo(k: ArcEnd, p: Point, sweep: number): { sweep: number; angle?: number; edit?: Edit } {
    const [c, q] = [k.center, p].map((v) => pageToLocal(frameOf(k.probe), k.probe.transform, v));
    let a = (Math.atan2(q[1] - c[1], q[0] - c[0]) * 180) / Math.PI;
    if (mods.angle) a = Math.round(a / 15) * 15;
    else if (!mods.free) a = Math.round(a);
    let d = k.end === "stop" ? a - k.start : k.stop - a;
    d += 360 * Math.round((sweep - d) / 360);
    d = Math.max(-360, Math.min(360, d));
    if (Math.abs(d) < 1e-6) return { sweep };
    const [start, stop] = k.end === "stop" ? [k.start, k.start + d] : [k.stop - d, k.stop];
    const values: Record<string, number> = { start, stop, delta: stop - start };
    const edits: Edit[] = k.keys.map((key) => ({ kind: "set-named", call: k.call, key, text: `${num(values[key])}deg` }));
    return { sweep: d, angle: k.end === "stop" ? stop : start, edit: { kind: "batch", edits } };
  }

  function reachAt(p: Point): Reach | undefined {
    return reaches.find((r) => Math.hypot(p[0] - r.point[0], p[1] - r.point[1]) * editor.zoom < 8);
  }

  /** The radius with the handle dragged to `p`: its start plus how far `p` went out from the centre. */
  function radiusAt(reach: Reach, p: Point): number {
    const [c, h, q] = [reach.center, reach.point, p].map((v) => pageToLocal(frameOf(reach.probe), reach.probe.transform, v));
    const len = Math.hypot(h[0] - c[0], h[1] - c[1]) || 1;
    const r = reach.r + ((q[0] - h[0]) * (h[0] - c[0]) + (q[1] - h[1]) * (h[1] - c[1])) / len;
    const snapping = editor.snap && !mods.free;
    return Math.max(snapping ? editor.snapValue(r) : r, snapping ? editor.gridStep : 0.01);
  }

  /** Counter-clockwise degrees of `p` around `c`, on the page. */
  function pageAngle(c: Point, p: Point): number {
    return (Math.atan2(c[1] - p[1], p[0] - c[0]) * 180) / Math.PI;
  }

  /** Degrees in (-180, 180]. */
  function wrapAngle(a: number): number {
    const w = ((a % 360) + 360) % 360;
    return w > 180 ? w - 360 : w;
  }

  /** Anchors you can snap to: top-level shapes outside loops. */
  const snapTargets = $derived(
    editor.probes.flatMap((probe) => {
      const call = editor.callById.get(probe.id);
      // A turned or scaled shape's scope lets its name through, so its anchors count too.
      const wrap = call && call.parent !== null ? editor.wrapper(call.id) : undefined;
      const topLevel = call && (call.parent === null || (wrap?.shape.id === call.id && wrap.scope.parent === null));
      if (!call || !topLevel || call.in_loop || probe.drawables.length === 0) return [];
      return Object.entries(probe.anchors)
        .filter(([, v]) => isVec(v))
        .map(([anchor, v]) => ({ target: probe.id, anchor, point: editor.toPage(frameOf(probe), v as [number, number]) }));
    }),
  );

  /** Literal vertices of top-level lines, which the join tool shares by naming them. */
  const vertexTargets = $derived(
    editor.scene.canvases.flatMap((canvas) =>
      canvas.calls.flatMap((call) => {
        const probe = probeOf.get(call.id);
        if (!probe || call.parent !== null || call.in_loop || baseName(call.callee) !== "line") return [];
        return call.args.flatMap((arg, i) =>
          arg.key === null && arg.point === null && arg.value.type === "coord"
            ? [{ vertex: { call: call.id, arg: i }, point: localToPage(probe, [arg.value.x, arg.value.y]) }]
            : [],
        );
      }),
    ),
  );

  /** How code names a point: `"A"` for an anchor, else its variable path. */
  function refText(id: number): string | undefined {
    const p = editor.pointById.get(id);
    if (!p) return undefined;
    if (p.anchors.length > 0) return JSON.stringify(p.anchors[0]);
    return p.path.replace(/\[(\d+)\]$/, ".at($1)");
  }

  /** Named points you can snap to. */
  const pointTargets = $derived(
    editor.scene.points.flatMap((p) => {
      const where = placed.get(p.id);
      const ref = refText(p.id);
      return where && ref ? [{ named: p.id, ref, point: where.page }] : [];
    }),
  );

  /** The canvas whose body holds `offset`, if any. */
  function canvasAt(offset: number) {
    return editor.scene.canvases.find((c) => c.body.start <= offset && offset < c.body.end);
  }

  /**
   * Whether code at `at` can use what's defined over `range`: CeTZ resolves
   * names in drawing order, so it must come first, in the same canvas
   * unless it's outside every canvas. `order: false` checks only the canvas.
   */
  function usableAt(range: Range, at: number, order = true): boolean {
    const canvas = canvasAt(range.start);
    return (!order || range.end <= at) && (!canvas || canvas === canvasAt(at));
  }

  /** Where a new shape goes: the end of the active canvas. */
  function insertAt(): number | undefined {
    const canvas = editor.scene.canvases.find((c) => c.id === editor.activeCanvas);
    return canvas && canvas.body.end - 1;
  }

  /**
   * The nearest snap target within reach. `exclude` skips anchors and
   * vertices of those shapes; `points: false` skips named points, `anchors: false` shape
   * anchors, and `vertices` adds lines' literal vertices. `at` is where the
   * reference will be written (an offset): only points and shapes defined
   * before it in its canvas count, though any vertex of that canvas does, as
   * sharing one names it above both lines. With `later`, shapes drawn after
   * it count too (connecting moves the call after them). Named points win
   * near-ties, since they're what you usually mean, then vertices; an anchor
   * within a few pixels of either (a path's `mid` near a corner) never wins.
   */
  function findSnap(
    p: Point,
    exclude?: number | Set<number>,
    {
      points = true,
      anchors = true,
      vertices = false,
      excludePoint,
      at,
      later = false,
    }: { points?: boolean; anchors?: boolean; vertices?: boolean; excludePoint?: number; at?: number; later?: boolean } = {},
  ): Snap | undefined {
    if (mods.free) return undefined;
    const radius = 8 / editor.zoom;
    let best: Snap | undefined;
    let bestDist = radius;
    const consider = (t: Snap, weight: number) => {
      const dist = Math.hypot(t.point[0] - p[0], t.point[1] - p[1]) * weight;
      if (dist < bestDist) {
        best = t;
        bestDist = dist;
      }
    };
    const excluded = (call: number) => (exclude instanceof Set ? exclude.has(call) : call === exclude);
    const usable = (range: Range | undefined, order = true) => at === undefined || (range !== undefined && usableAt(range, at, order));
    const shadow = 4 / editor.zoom;
    const corners: Snap[] = [];
    const corner = (t: Snap, weight: number) => {
      if (Math.hypot(t.point[0] - p[0], t.point[1] - p[1]) < radius + shadow) corners.push(t);
      consider(t, weight);
    };
    if (points) for (const t of pointTargets) if (t.named !== excludePoint && usable(editor.pointById.get(t.named)?.range)) corner(t, 0.75);
    if (vertices) for (const t of vertexTargets) if (!excluded(t.vertex.call) && usable(editor.callById.get(t.vertex.call)?.range, false)) corner(t, 0.85);
    if (anchors) {
      const shadowed = (q: Point) => corners.some((c) => Math.hypot(c.point[0] - q[0], c.point[1] - q[1]) < shadow);
      for (const t of snapTargets) {
        // With `later`, shapes drawn after `at` count too: connecting moves the call after them.
        if (excluded(t.target) || !usable(editor.callById.get(t.target)?.range, !later)) continue;
        if (shadowed(t.point)) continue;
        // Plain compass anchors (`east`) beat near-identical text ones (`base-east`, `mid-east`).
        consider(t, /^(base|mid)/.test(t.anchor) ? 1.15 : 1);
      }
    }
    return best;
  }

  // --- Snapping modifiers ----------------------------------------------------

  const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
  /**
   * ⌘ (Ctrl off macOS) places freely; Shift locks segments to 15° steps;
   * ⌥ joins onto points as literal coordinates, not shared ones. Read from
   * each pointer event.
   */
  let mods = $state({ free: false, angle: false, detach: false });
  function readMods(e: PointerEvent | KeyboardEvent) {
    mods = { free: isMac ? e.metaKey : e.ctrlKey, angle: e.shiftKey, detach: e.altKey };
  }

  /**
   * Locks `to` onto the nearest 15° direction from `from` (page points),
   * keeping its distance along that direction.
   */
  function constrainAngle(from: Point, to: Point): Point {
    const dx = to[0] - from[0];
    const dy = to[1] - from[1];
    const step = Math.PI / 12;
    const angle = Math.round(Math.atan2(dy, dx) / step) * step;
    const length = dx * Math.cos(angle) + dy * Math.sin(angle);
    return [from[0] + length * Math.cos(angle), from[1] + length * Math.sin(angle)];
  }

  /**
   * A point for the next vertex from `from` with Shift held: on a 15° ray,
   * with the length snapped to the grid along horizontal and vertical rays.
   */
  function angled(frame: Frame, transform: number[][] | undefined, from: Point, p: Point): { page: Point; local: Point } {
    const page = constrainAngle(from, p);
    let local = pageToLocal(frame, transform, page);
    if (!mods.free) {
      const start = pageToLocal(frame, transform, from);
      if (Math.abs(page[1] - from[1]) < 1e-9) local = [start[0] + editor.snapValue(local[0] - start[0]), start[1]];
      else if (Math.abs(page[0] - from[0]) < 1e-9) local = [start[0], start[1] + editor.snapValue(local[1] - start[1])];
    }
    return { page: editor.toPage(frame, transformPoint(transform, local)), local };
  }

  /**
   * The edit that points one argument at a snap target. `map` follows the
   * target's id through earlier edits of a chain.
   */
  function snapEdit(call: number, arg: number, snap: Snap, map = (id: number) => id): Edit | undefined {
    if (snap.ref !== undefined) return { kind: "set-arg-text", call, arg, text: snap.ref };
    if (snap.vertex !== undefined) return { kind: "share-point", call, arg, from: map(snap.vertex.call), from_arg: snap.vertex.arg };
    if (snap.target !== undefined && snap.anchor !== undefined) return { kind: "connect", call, arg, target: map(snap.target), anchor: snap.anchor };
    return undefined;
  }

  // --- Pointer interaction -------------------------------------------------

  function snapPoint(frame: Frame, transform: number[][] | undefined, p: Point): Point {
    const [x, y] = pageToLocal(frame, transform, p);
    return mods.free ? [x, y] : [editor.snapValue(x), editor.snapValue(y)];
  }

  // --- Smart guides -----------------------------------------------------------

  type Box = { x0: number; y0: number; x1: number; y1: number };
  /** A guide on the page: vertical at `x` or horizontal at `y`, running `from` to `to` along the other axis. */
  type Guide = { x?: number; y?: number; from: number; to: number };
  let guides = $state<Guide[]>([]);
  /** Other shapes' bounds for the drag in progress, worked out on first use. */
  let guideCache: Box[] | undefined;

  /** The page bounds of the shapes you can click, other than `exclude` and what's in them. */
  function guideTargets(exclude: number[]): Box[] {
    if (guideCache) return guideCache;
    const skip = new Set(exclude.flatMap((id) => [...editor.family(id)]));
    guideCache = editor.calls.filter((c) => editor.isSelectable(c) && !skip.has(c.id)).flatMap((c) => editor.boundsOf(c.id) ?? []);
    return guideCache;
  }

  /**
   * How far to shift `box` (page) so its left, centre or right lines up with
   * another shape's, and likewise top, middle or bottom, within a few pixels;
   * and the guides that show it. A point is a box with no size.
   */
  function alignBox(box: Box, targets: Box[]): { dx?: number; dy?: number; lines: Guide[] } {
    const reach = 6 / editor.zoom;
    const nearest = (lo: "x0" | "y0", hi: "x1" | "y1") => {
      let best: { shift: number; at: number; t: Box } | undefined;
      const mine = [box[lo], (box[lo] + box[hi]) / 2, box[hi]];
      for (const t of targets) {
        for (const at of [t[lo], (t[lo] + t[hi]) / 2, t[hi]]) {
          for (const m of mine) {
            const shift = at - m;
            if (Math.abs(shift) <= reach && (!best || Math.abs(shift) < Math.abs(best.shift))) best = { shift, at, t };
          }
        }
      }
      return best;
    };
    const [bx, by] = [nearest("x0", "x1"), nearest("y0", "y1")];
    const [sx, sy] = [bx?.shift ?? 0, by?.shift ?? 0];
    // Each guide runs across both shapes, where they'll be.
    const lines: Guide[] = [];
    if (bx) lines.push({ x: bx.at, from: Math.min(box.y0 + sy, bx.t.y0), to: Math.max(box.y1 + sy, bx.t.y1) });
    if (by) lines.push({ y: by.at, from: Math.min(box.x0 + sx, by.t.x0), to: Math.max(box.x1 + sx, by.t.x1) });
    return { dx: bx?.shift, dy: by?.shift, lines };
  }

  function shiftBox(b: Box, [dx, dy]: Point): Box {
    return { x0: b.x0 + dx, y0: b.y0 + dy, x1: b.x1 + dx, y1: b.y1 + dy };
  }

  /** Like `snapPoint`, but on each axis an edge or centre of another shape within reach wins over the grid. */
  function snapPointGuided(frame: Frame, transform: number[][] | undefined, p: Point, exclude: number[]): Point {
    const grid = snapPoint(frame, transform, p);
    if (mods.free) {
      guides = [];
      return grid;
    }
    const g = alignBox({ x0: p[0], y0: p[1], x1: p[0], y1: p[1] }, guideTargets(exclude));
    guides = g.lines;
    if (g.dx === undefined && g.dy === undefined) return grid;
    const onGrid = editor.toPage(frame, transformPoint(transform, grid));
    const local = pageToLocal(frame, transform, [g.dx !== undefined ? p[0] + g.dx : onGrid[0], g.dy !== undefined ? p[1] + g.dy : onGrid[1]]);
    return local.map((v) => Math.round(v * 1e4) / 1e4) as Point;
  }

  /** The frame and transform new shapes are drawn in: the end of the active canvas. */
  function creationFrame(): { frame: Frame; transform?: number[][] } {
    const canvas = editor.activeCanvas;
    const last = [...editor.probes].reverse().find((p) => editor.canvasOfCall.get(p.id) === canvas);
    return { frame: editor.frameFor(canvas), transform: last?.transform };
  }

  // --- Point and join tools -----------------------------------------------

  /** Where the point/join tools would place their next point, while hovering. */
  let toolHover = $state<{ page: Point; snap?: Snap }>();

  /**
   * A path being joined point by point: each step's source text and page
   * position, plus snaps to apply once it exists (an unnamed shape's anchor
   * to connect, another line's vertex to share). With `extend`, the steps
   * continue an existing line from one of its ends instead.
   */
  type Joining = { refs: string[]; pages: Point[]; links: { arg: number; snap: Snap }[]; extend?: Extending };
  /**
   * Continuing line `call` from its first vertex (`start`) or last: where
   * that end is, where the other end is (clicking it closes the line), and
   * the frame new vertices are written in.
   */
  type Extending = { call: number; start: boolean; from: Point; other: Point; frame: Frame; transform?: number[][] };
  let joining = $state<Joining>();

  /** Where the next joined segment starts: the last step, or the end being continued. */
  function lastJoined(path: Joining | undefined): Point | undefined {
    return path?.pages[path.pages.length - 1] ?? path?.extend?.from;
  }

  /**
   * A path whose points can be edited: a plain `line(..)`, or a curve
   * through points (`catmull`, `hobby`), outside loops. `verts` are its
   * positional arguments' indices; `tool` is what continues it.
   */
  function editablePath(id: number) {
    const call = editor.callById.get(id);
    const probe = probeOf.get(id);
    const base = call && baseName(call.callee);
    if (!call || !probe || call.in_loop || !base || !["line", "catmull", "hobby"].includes(base)) return undefined;
    const verts = call.args.flatMap((a, i) => (a.key === null ? [i] : []));
    const closed = call.args.some((a) => a.key === "close" && a.text.trim() === "true");
    const tool: Tool = base === "line" ? "join" : "curve";
    return { call, probe, verts, closed, tool };
  }

  /** Starts continuing an open line from its first or last vertex with the join tool. */
  function continueLine(id: number, start: boolean) {
    const path = editablePath(id);
    if (!path || path.closed || path.verts.length < 2) return;
    const [first, last] = [path.verts[0], path.verts[path.verts.length - 1]];
    const from = argHandle(path.call, path.probe, start ? first : last)?.point;
    const other = argHandle(path.call, path.probe, start ? last : first)?.point;
    if (!from || !other) return;
    editor.selection = [id];
    editor.pointSelection = [];
    editor.tool = path.tool;
    const extend = { call: id, start, from, other, frame: frameOf(path.probe), transform: path.probe.transform };
    joining = { refs: [], pages: [], links: [], extend };
  }

  /** The open selected path the join (or curve) tool would continue, before its first click. */
  const continuable = $derived.by(() => {
    if (!isJoinTool() || joining || editor.selected.length !== 1) return undefined;
    const path = editablePath(editor.selected[0]);
    return path && path.tool === editor.tool && !path.closed && path.verts.length >= 2 ? path : undefined;
  });

  /** The end of the continuable line near `p`, if any. */
  function selectedEndAt(p: Point): { call: number; start: boolean } | undefined {
    const path = continuable;
    if (!path) return undefined;
    const ends = [path.verts[0], path.verts[path.verts.length - 1]];
    for (const [k, arg] of ends.entries()) {
      const at = argHandle(path.call, path.probe, arg)?.point;
      if (at && Math.hypot(at[0] - p[0], at[1] - p[1]) * editor.zoom < 8) return { call: path.call.id, start: k === 0 };
    }
    return undefined;
  }

  /**
   * Where on a curve through points `p` is nearest: the point on the drawn
   * curve, and the argument to insert it before. Its outline has one cubic
   * per pair of points, so the nearest cubic says which pair it's between.
   */
  function onCurve(path: NonNullable<ReturnType<typeof editablePath>>, p: Point): { at: number; q: Point } | undefined {
    const outline = path.probe.drawables.find((d) => d.type === "path");
    const sub = outline?.type === "path" ? outline.segments[0] : undefined;
    const n = path.verts.length;
    if (!sub || sub[2].length !== (path.closed ? n : n - 1) || sub[2].some((s) => s[0] !== "c")) return undefined;
    const page = (v: Vec3) => editor.toPage(frameOf(path.probe), v);
    let start = sub[0];
    let best: { k: number; q: Point; dist: number } | undefined;
    for (const [k, seg] of sub[2].entries()) {
      const [, c1, c2, end] = seg as ["c", Vec3, Vec3, Vec3];
      const [a, b, c, d] = [start, c1, c2, end].map(page);
      for (let i = 0; i <= 32; i++) {
        const t = i / 32;
        const w = [(1 - t) ** 3, 3 * (1 - t) ** 2 * t, 3 * (1 - t) * t ** 2, t ** 3];
        const q: Point = [w[0] * a[0] + w[1] * b[0] + w[2] * c[0] + w[3] * d[0], w[0] * a[1] + w[1] * b[1] + w[2] * c[1] + w[3] * d[1]];
        const dist = Math.hypot(q[0] - p[0], q[1] - p[1]);
        if (!best || dist < best.dist) best = { k, q, dist };
      }
      start = end;
    }
    return best && { at: best.k + 1 < n ? path.verts[best.k + 1] : path.verts[best.k] + 1, q: best.q };
  }

  /** Inserts a vertex on the path where it's nearest `p` (for a line, on the nearest segment), on the grid when snapping. */
  function addVertexAt(id: number, p: Point) {
    const path = editablePath(id);
    if (!path) return;
    const pages = path.verts.map((arg) => argHandle(path.call, path.probe, arg)?.point);
    // A curve's own outline places the point; failing that, the straight segments between its points.
    const curve = path.tool === "curve" ? onCurve(path, p) : undefined;
    let best: { at: number; q: Point; dist: number } | undefined = curve && { ...curve, dist: 0 };
    const count = curve ? 0 : path.closed ? pages.length : pages.length - 1;
    for (let k = 0; k < count; k++) {
      const [a, b] = [pages[k], pages[(k + 1) % pages.length]];
      if (!a || !b) continue;
      const [dx, dy] = [b[0] - a[0], b[1] - a[1]];
      const t = Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy || 1)));
      const q: Point = [a[0] + t * dx, a[1] + t * dy];
      const dist = Math.hypot(q[0] - p[0], q[1] - p[1]);
      // The closing segment's new vertex goes after the last one.
      if (!best || dist < best.dist) best = { at: k + 1 < pages.length ? path.verts[k + 1] : path.verts[k] + 1, q, dist };
    }
    if (!best) return;
    const [x, y] = pageToLocal(frameOf(path.probe), path.probe.transform, best.q).map((v) => editor.snapValue(v));
    editor.edit({ kind: "insert-args", call: id, at: best.at, texts: [`(${num(x)}, ${num(y)})`] });
  }

  // --- Context menu --------------------------------------------------------

  type MenuItem = { label: string; keys?: string; run: () => void };
  /** The right-click menu, at screen pixels in the viewport: sections of items, divided by rules. */
  let menu = $state<{ at: Point; sections: MenuItem[][] }>();
  const modKey = isMac ? "⌘" : "Ctrl+";
  const shiftModKey = isMac ? "⇧⌘" : "Ctrl+Shift+";

  /**
   * Right-click belongs to the canvas: what's offered depends on what's
   * under the pointer. A shape outside the selection becomes the selection.
   */
  function oncontextmenu(e: MouseEvent) {
    e.preventDefault();
    if (drag) return;
    const sections = joining ? joinMenu(joining) : editor.tool === "select" ? selectMenu(e) : [];
    const shown = sections.filter((items) => items.length > 0);
    if (shown.length === 0) return;
    const r = viewport.getBoundingClientRect();
    menu = { at: [e.clientX - r.left, e.clientY - r.top], sections: shown };
  }

  function joinMenu(path: Joining): MenuItem[][] {
    const canClose = path.extend ? path.pages.length >= 1 : path.pages.length >= 3;
    const canFinish = path.pages.length >= (path.extend ? 1 : 2);
    return [
      [
        ...(canFinish ? [{ label: "Finish line", keys: "↩", run: () => finishJoin(path, false) }] : []),
        ...(canClose ? [{ label: "Close path", run: () => finishJoin(path, true) }] : []),
        { label: "Cancel", keys: "Esc", run: () => (joining = undefined) },
      ],
    ];
  }

  function selectMenu(e: MouseEvent): MenuItem[][] {
    const p = pagePoint(e);
    const grab = grabAt(p);
    if (grab && "handle" in grab) return [vertexItems(grab.handle.call, grab.handle.arg)];
    if (grab && "point" in grab) return pointMenu(grab.point);
    const hit = (e.target as Element).closest("[data-id]")?.getAttribute("data-id");
    if (hit === null || hit === undefined) return canvasMenu();
    const id = Number(hit);
    if (!editor.selected.includes(id)) {
      editor.selection = [id];
      editor.pointSelection = [];
    }
    const selected = editor.selected;
    const lineItems = selected.length === 1 ? pathItems(id, p) : [];
    return [
      lineItems,
      [
        { label: "Duplicate", keys: `${modKey}D`, run: () => editor.edit({ kind: "duplicate", calls: editor.selected, dx: 0.5, dy: -0.5 }) },
        ...(selected.length > 1 ? [{ label: "Group", keys: `${modKey}G`, run: () => editor.groupSelection() }] : []),
        ...(selected.some((s) => editor.isGroup(s)) ? [{ label: "Ungroup", keys: `${shiftModKey}G`, run: () => editor.ungroupSelection() }] : []),
      ],
      [
        { label: "Bring to front", keys: `${shiftModKey}]`, run: () => editor.arrangeSelection("front") },
        { label: "Bring forward", keys: `${modKey}]`, run: () => editor.arrangeSelection("forward") },
        { label: "Send backward", keys: `${modKey}[`, run: () => editor.arrangeSelection("backward") },
        { label: "Send to back", keys: `${shiftModKey}[`, run: () => editor.arrangeSelection("back") },
      ],
      [{ label: "Delete", keys: "⌫", run: () => editor.deleteSelection() }],
    ];
  }

  /** A path's own items: add a point where you clicked, continue it, close or open it. */
  function pathItems(id: number, p: Point): MenuItem[] {
    const path = editablePath(id);
    if (!path) return [];
    const items: MenuItem[] = [{ label: "Add point here", run: () => addVertexAt(id, p) }];
    if (!path.closed) {
      items.push({ label: "Continue from start", run: () => continueLine(id, true) });
      items.push({ label: "Continue from end", run: () => continueLine(id, false) });
    }
    if (path.closed || path.verts.length >= 3) {
      items.push({
        label: path.closed ? "Open path" : "Close path",
        run: () => editor.edit({ kind: "set-named", call: id, key: "close", text: path.closed ? null : "true" }),
      });
    }
    return items;
  }

  /** One of a path's vertices: continue from it if it's an end, or remove it. */
  function vertexItems(call: number, arg: number): MenuItem[] {
    const path = editablePath(call);
    if (!path) return [];
    const items: MenuItem[] = [];
    const last = path.verts[path.verts.length - 1];
    if (!path.closed && (arg === path.verts[0] || arg === last)) {
      items.push({ label: "Continue from here", run: () => continueLine(call, arg === path.verts[0]) });
    }
    const keep = path.closed ? 3 : 2;
    if (path.verts.length > keep) items.push({ label: "Remove point", run: () => editor.edit({ kind: "remove-arg", call, arg, keep }) });
    return items;
  }

  function pointMenu(point: number): MenuItem[][] {
    editor.selection = [];
    editor.pointSelection = [point];
    const users = [...new Set((editor.pointUsers.get(point) ?? []).map((id) => editor.selectableFor(id)))].filter((id) => id !== undefined);
    return [
      [
        { label: "Rename", run: () => (editor.renamingPoint = point) },
        ...(users.length ? [{ label: "Select shapes using it", run: () => ((editor.pointSelection = []), (editor.selection = users)) }] : []),
      ],
      [{ label: "Delete", keys: "⌫", run: () => editor.deleteSelection() }],
    ];
  }

  function canvasMenu(): MenuItem[][] {
    return [
      [
        { label: "Select all", keys: `${modKey}A`, run: () => (editor.selection = editor.calls.filter((c) => editor.isSelectable(c)).map((c) => c.id)) },
        { label: "Zoom to fit", keys: `${modKey}0`, run: fit },
      ],
      ...(editor.calls.some((c) => baseName(c.callee) === "anchor") ? [[{ label: "Gather anchors at top", run: () => editor.gatherAnchors() }]] : []),
      [
        { label: editor.showPoints ? "Hide points" : "Show points", keys: "P", run: () => (editor.showPoints = !editor.showPoints) },
        { label: editor.showGrid ? "Hide grid" : "Show grid", keys: "G", run: () => (editor.showGrid = !editor.showGrid) },
      ],
    ];
  }

  // Leaving the join tool abandons a half-made path.
  $effect(() => {
    if (!isJoinTool()) joining = undefined;
    if (editor.tool !== "point" && !isJoinTool() && !isLineTool()) toolHover = undefined;
  });

  /**
   * Where the point/join tools put their next point: with Shift, on a 15°
   * ray from the previous one; else on a named point or (join only) a line
   * vertex or shape anchor; else the grid. ⌘ skips all snapping.
   */
  function toolTarget(p: Point, join: boolean): { page: Point; local: Point; snap?: Snap } {
    const extend = join ? joining?.extend : undefined;
    const { frame, transform } = extend ?? creationFrame();
    const last = lastJoined(joining);
    if (join && mods.angle && last) return angled(frame, transform, last, p);
    const at = extend ? editor.callById.get(extend.call)?.range.start : insertAt();
    const snap = join ? findSnap(p, extend?.call, { vertices: true, at }) : undefined;
    if (snap) return { page: snap.point, local: pageToLocal(frame, transform, snap.point), snap };
    const local = snapPoint(frame, transform, p);
    return { page: editor.toPage(frame, transformPoint(transform, local)), local };
  }

  function placePoint(p: Point) {
    const { local } = toolTarget(p, false);
    if (!editor.edit({ kind: "add-point", canvas: editor.activeCanvas ?? null, x: local[0], y: local[1], name: null })) return;
    // The edit reports the new point as "created"; it isn't a shape to select.
    const point = editor.selection[0];
    editor.selection = [];
    editor.pointSelection = [point];
    editor.tool = "select";
    editor.renamingPoint = point;
  }

  function joinAt(p: Point, double: boolean) {
    // Starting on an end of the selected line continues it.
    const end = joining ? undefined : selectedEndAt(p);
    if (end) return continueLine(end.call, end.start);
    const path = joining ?? { refs: [], pages: [], links: [] };
    const { extend } = path;
    // Closing on the first point is checked against the raw pointer, so an
    // angle-locked ray doesn't stop you landing on it.
    const near = (a: Point, b: Point) => Math.hypot(a[0] - b[0], a[1] - b[1]) * editor.zoom < 8;
    if (extend ? path.pages.length >= 1 && near(extend.other, p) : path.pages.length >= 3 && near(path.pages[0], p)) return finishJoin(path, true);
    if (double && path.pages.length >= (extend ? 1 : 2)) return finishJoin(path, false);
    const target = toolTarget(p, true);
    const last = lastJoined(path);
    if (last && near(last, target.page)) return;
    const literal = `(${num(target.local[0])}, ${num(target.local[1])})`;
    // With ⌥ the point lands where it snapped but stays a literal.
    const snap = mods.detach ? undefined : target.snap;
    let ref = snap?.ref ?? literal;
    let links = path.links;
    // A shape's anchor goes by name if it has one; anything else links up once the path exists.
    const name = snap?.target !== undefined ? editor.callById.get(snap.target)?.name : undefined;
    if (name && snap?.anchor !== undefined) ref = JSON.stringify(`${name}.${snap.anchor}`);
    else if (snap && snap.ref === undefined) links = [...links, { arg: path.refs.length, snap }];
    joining = { ...path, refs: [...path.refs, ref], pages: [...path.pages, target.page], links };
    // An angle mark is done at its third point.
    if (editor.tool === "angle" && joining.refs.length === 3) finishJoin(joining, false);
  }

  function finishJoin(path: Joining, closed: boolean) {
    joining = undefined;
    if (path.extend) return finishExtend(path, path.extend, closed);
    if (editor.tool === "angle") return finishAngle(path);
    if (path.refs.length < 2) return;
    const text = `${editor.tool === "curve" ? "catmull" : "line"}(${path.refs.join(", ")}${closed ? ", close: true" : ""})`;
    const steps: Parameters<Editor["chain"]>[0] = [{ kind: "insert", canvas: editor.activeCanvas ?? null, text }];
    for (const { arg, snap } of path.links) steps.push(({ created, map }) => snapEdit(created[0], arg, snap, map));
    if (editor.chain(steps)) editor.tool = "select";
  }

  /**
   * Writes an angle mark from its corner and a point on each side. CeTZ
   * sweeps counter-clockwise from the first side, so the sides go in the
   * order that marks the inner angle; square sides get `right-angle`.
   */
  function finishAngle(path: Joining) {
    if (path.refs.length !== 3) return;
    const [o, a, b] = path.pages;
    const [u, v] = [[a[0] - o[0], a[1] - o[1]], [b[0] - o[0], b[1] - o[1]]];
    // The page's y runs down, so a clockwise turn here is counter-clockwise in CeTZ.
    const swap = u[0] * v[1] - u[1] * v[0] > 0;
    const right = Math.abs(u[0] * v[0] + u[1] * v[1]) < 1e-3 * Math.hypot(...u) * Math.hypot(...v);
    const order = swap ? [0, 2, 1] : [0, 1, 2];
    const text = `${right ? "right-angle" : "angle"}(${order.map((i) => path.refs[i]).join(", ")})`;
    const steps: Parameters<Editor["chain"]>[0] = [{ kind: "insert-library", canvas: editor.activeCanvas ?? null, module: "angle", text }];
    for (const { arg, snap } of path.links) steps.push(({ created, map }) => snapEdit(created[0], order.indexOf(arg), snap, map));
    if (editor.chain(steps)) editor.tool = "select";
  }

  /** Adds the joined steps to the end of the line being continued, and closes it if asked. */
  function finishExtend(path: Joining, extend: Extending, closed: boolean) {
    const line = editablePath(extend.call);
    const n = path.refs.length;
    if (!line || n === 0) return;
    // From the start, the steps go in before the first vertex, nearest it last.
    const at = extend.start ? line.verts[0] : line.verts[line.verts.length - 1] + 1;
    const texts = extend.start ? [...path.refs].reverse() : path.refs;
    const argOf = (k: number) => (extend.start ? at + n - 1 - k : at + k);
    const steps: Parameters<Editor["chain"]>[0] = [{ kind: "insert-args", call: extend.call, at, texts }];
    for (const { arg, snap } of path.links) steps.push(({ map }) => snapEdit(map(extend.call), argOf(arg), snap, map));
    if (closed) steps.push(({ map }) => ({ kind: "set-named", call: map(extend.call), key: "close", text: "true" }));
    if (editor.chain(steps)) editor.tool = "select";
  }

  const joinPreview = $derived.by(() => {
    if (!joining) return undefined;
    const from = joining.extend ? [joining.extend.from] : [];
    let pages = [...from, ...joining.pages, ...(toolHover ? [toolHover.page] : [])];
    if (pages.length < 2) return undefined;
    // An angle's sides both run from its corner, the first point.
    if (editor.tool === "angle" && pages.length >= 2) pages = [pages[1], pages[0], ...pages.slice(2)];
    if (editor.tool !== "curve") return pages.map((q, i) => `${i ? "L" : "M"}${q[0]},${q[1]}`).join(" ");
    // A Catmull-Rom spline through the points, as cubic Béziers.
    const at = (i: number) => pages[Math.max(0, Math.min(pages.length - 1, i))];
    let d = `M${pages[0][0]},${pages[0][1]}`;
    for (let i = 0; i < pages.length - 1; i++) {
      const [p0, p1, p2, p3] = [at(i - 1), at(i), at(i + 1), at(i + 2)];
      const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
      const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
      d += ` C${c1[0]},${c1[1]} ${c2[0]},${c2[1]} ${p2[0]},${p2[1]}`;
    }
    return d;
  });

  function onpointerdown(e: PointerEvent) {
    menu = undefined;
    guideCache = undefined;
    if (e.button === 2) return;
    viewport.setPointerCapture(e.pointerId);
    const p = pagePoint(e);
    readMods(e);

    if (e.button === 0 && !spaceHeld && editor.tool === "point") {
      placePoint(p);
      return;
    }
    if (e.button === 0 && !spaceHeld && isJoinTool()) {
      joinAt(p, e.detail >= 2);
      return;
    }
    if (e.button === 0 && !spaceHeld && arcing) {
      arcClick(arcing, p);
      return;
    }

    if (e.button === 1 || spaceHeld) {
      drag = { kind: "pan", start: [e.clientX, e.clientY], pan: [...editor.pan] };
      return;
    }

    if (editor.tool !== "select") {
      const { frame, transform } = creationFrame();
      const startSnap = findSnap(p, undefined, { anchors: isLineTool(), vertices: isLineTool(), at: insertAt() });
      const start = startSnap ? pageToLocal(frame, transform, startSnap.point) : snapPoint(frame, transform, p);
      drag = { kind: "create", start, end: start, startSnap, frame, transform };
      toolHover = undefined;
      return;
    }

    const reshape = reshapeAt(p);
    if (reshape) {
      drag = { kind: "reshape", reshape };
      return;
    }
    const arcEnd = arcEndAt(p);
    if (arcEnd) {
      drag = { kind: "sweep", end: arcEnd, sweep: arcEnd.stop - arcEnd.start };
      return;
    }
    const reach = reachAt(p);
    if (reach) {
      drag = { kind: "radius", reach, r: reach.r };
      return;
    }
    if (grow && growKnob && overGrow(p)) {
      drag = { kind: "grow", grow, from: Math.hypot(p[0] - grow.pivot[0], p[1] - grow.pivot[1]), factor: grow.factor };
      return;
    }
    if (spin && overSpin(p)) {
      drag = { kind: "rotate", spin, from: pageAngle(spin.pivot, p), angle: spin.angle };
      return;
    }

    const target = e.target as Element;
    const grab = grabAt(p);
    if (grab && "handle" in grab) {
      const { call, arg } = grab.handle;
      const probe = probeOf.get(call);
      const point = editor.callById.get(call)?.args[arg]?.point ?? null;
      const where = point !== null ? placed.get(point) : undefined;
      if (probe && point !== null && where) {
        drag = { kind: "point", point, start: p, moved: false, frame: where.frame, transform: where.transform, use: { call, arg, probe }, detach: e.altKey };
      } else if (probe) {
        drag = { kind: "handle", call, arg, probe };
      }
      return;
    }

    if (grab && "point" in grab) {
      const { point } = grab;
      const picked = editor.selectedPoints.includes(point);
      if (e.shiftKey) {
        editor.pointSelection = picked ? editor.selectedPoints.filter((id) => id !== point) : [...editor.selectedPoints, point];
        return;
      }
      if (!picked) {
        editor.selection = [];
        editor.pointSelection = [point];
      }
      const group = editor.selectedPoints.filter((id) => id !== point);
      const where = placed.get(point);
      if (where) drag = { kind: "point", point, start: p, moved: false, frame: where.frame, transform: where.transform, detach: false, group };
      return;
    }

    const hit = target.closest("[data-id]")?.getAttribute("data-id");
    if (hit !== null && hit !== undefined) {
      const id = Number(hit);
      if (e.shiftKey) {
        editor.selection = editor.selected.includes(id) ? editor.selected.filter((s) => s !== id) : [...editor.selected, id];
      } else if (!editor.selected.includes(id)) {
        editor.selection = [id];
        editor.pointSelection = [];
      }
      if (editor.selected.includes(id)) drag = { kind: "move", start: p, moved: false, delta: [0, 0], detach: e.altKey, box: selectionBox && { ...selectionBox } };
      return;
    }

    const base = e.shiftKey ? [...editor.selected] : [];
    const basePoints = e.shiftKey ? [...editor.selectedPoints] : [];
    if (!e.shiftKey) {
      editor.selection = [];
      editor.pointSelection = [];
    }
    drag = { kind: "marquee", start: p, end: p, base, basePoints };
  }

  function onpointermove(e: PointerEvent) {
    const p = pagePoint(e);
    readMods(e);
    const r = viewport.getBoundingClientRect();
    pointer = [e.clientX - r.left, e.clientY - r.top];
    if (!drag && arcing) {
      arcMove(arcing, p);
      return;
    }
    if (!drag && (editor.tool === "point" || isJoinTool())) {
      toolHover = toolTarget(p, isJoinTool());
      return;
    }
    // Line tools preview where a press would start: just the snap, no ghost.
    if (!drag && isLineTool()) {
      const snap = findSnap(p, undefined, { vertices: true, at: insertAt() });
      toolHover = snap && { page: snap.point, snap };
    }
    if (!drag) {
      // A point in reach takes the pointer from the shape under it.
      nearArcEnd = editor.tool === "select" && !spaceHeld && arcEndAt(p) !== undefined;
      nearReshape = editor.tool === "select" && !spaceHeld && !nearArcEnd && reshapeAt(p) !== undefined;
      nearReach = editor.tool === "select" && !spaceHeld && !nearReshape && reachAt(p) !== undefined;
      nearSpin = editor.tool === "select" && !spaceHeld && !nearReach && overSpin(p);
      nearGrow = editor.tool === "select" && !spaceHeld && !nearReach && !nearSpin && overGrow(p);
      nearGrab = editor.tool === "select" && !spaceHeld && !nearSpin && !nearGrow && !nearReach && !nearReshape && !nearArcEnd ? grabAt(p) : undefined;
      const hit = nearGrab || nearSpin || nearGrow || nearReach || nearReshape || nearArcEnd ? null : (e.target as Element).closest("[data-id]")?.getAttribute("data-id");
      editor.hoverSource = "canvas";
      editor.hovered = hit ? Number(hit) : undefined;
      editor.hoveredPoint = nearGrab ? ("point" in nearGrab ? nearGrab.point : nearGrab.handle.shared) : undefined;
      return;
    }
    switch (drag.kind) {
      case "pan":
        editor.pan = [drag.pan[0] + e.clientX - drag.start[0], drag.pan[1] + e.clientY - drag.start[1]];
        break;
      case "move": {
        const probe = probeOf.get(editor.selected[0]);
        if (!probe) break;
        const dist = Math.hypot(p[0] - drag.start[0], p[1] - drag.start[1]) * editor.zoom;
        if (!drag.moved && dist < 3) break;
        drag.moved = true;
        const [dx, dy] = untransformDelta(probe.transform, [
          (p[0] - drag.start[0]) / probe.length,
          -(p[1] - drag.start[1]) / probe.length,
        ]);
        drag.delta = snapDelta(dx, dy);
        // Line the selection's bounds up with other shapes', per axis, over the grid.
        const raw: Point = [p[0] - drag.start[0], p[1] - drag.start[1]];
        const g = drag.box && !mods.free ? alignBox(shiftBox(drag.box, raw), guideTargets(editor.selected)) : undefined;
        guides = g?.lines ?? [];
        if (g && (g.dx !== undefined || g.dy !== undefined)) {
          const m = probe.transform;
          const [wx, wy] = m ? [m[0][0] * drag.delta[0] + m[0][1] * drag.delta[1], m[1][0] * drag.delta[0] + m[1][1] * drag.delta[1]] : drag.delta;
          const want = [g.dx !== undefined ? raw[0] + g.dx : wx * probe.length, g.dy !== undefined ? raw[1] + g.dy : -wy * probe.length];
          const local = untransformDelta(m, [want[0] / probe.length, -want[1] / probe.length]);
          drag.delta = local.map((v) => Math.round(v * 1e4) / 1e4) as Point;
        }
        drag.detach = e.altKey;
        editor.dragMove(drag.delta[0], drag.delta[1], drag.detach);
        break;
      }
      case "point": {
        if (!drag.moved && Math.hypot(p[0] - drag.start[0], p[1] - drag.start[1]) * editor.zoom < 3) break;
        drag.moved = true;
        drag.detach = e.altKey && drag.use !== undefined;
        if (drag.detach && drag.use) {
          // Detach this use: like a plain coordinate handle.
          const { call, arg, probe } = drag.use;
          const snap = findSnap(p, call, { vertices: true, at: editor.callById.get(call)?.range.start, later: true });
          const edit = snap && snapEdit(call, arg, snap);
          // A snap the edit refuses (a vertex it can't share this early) falls back to the grid.
          if (edit && editor.previewEdit(edit)) {
            drag.snap = snap;
            drag.edit = edit;
          } else {
            const [x, y] = snapPointGuided(frameOf(probe), probe.transform, p, [call]);
            drag.snap = undefined;
            drag.edit = { kind: "set-coord", call, arg, x, y };
            editor.previewEdit(drag.edit);
          }
        } else {
          // Move the shared point itself; snap it onto other points, or anchors of shapes that don't use it.
          const snap = findSnap(p, new Set(editor.pointUsers.get(drag.point) ?? []), { excludePoint: drag.point });
          drag.snap = snap;
          if (snap) guides = [];
          const [x, y] = snap ? pageToLocal(drag.frame, drag.transform, snap.point) : snapPointGuided(drag.frame, drag.transform, p, editor.pointUsers.get(drag.point) ?? []);
          const own = editor.pointById.get(drag.point);
          drag.edit =
            drag.group?.length && own
              ? { kind: "move-points", points: [drag.point, ...drag.group], dx: x - own.x, dy: y - own.y }
              : { kind: "set-point", point: drag.point, x, y };
          editor.previewEdit(drag.edit);
        }
        break;
      }
      case "handle": {
        const snap = findSnap(p, drag.call, { vertices: true, at: editor.callById.get(drag.call)?.range.start, later: true });
        const edit = snap && snapEdit(drag.call, drag.arg, snap);
        // A snap the edit refuses (a vertex it can't share this early) falls back to the grid.
        if (edit && editor.previewEdit(edit)) {
          drag.snap = snap;
          drag.edit = edit;
          guides = [];
        } else {
          const [x, y] = snapPointGuided(frameOf(drag.probe), drag.probe.transform, p, [drag.call]);
          drag.snap = undefined;
          drag.edit = { kind: "set-coord", call: drag.call, arg: drag.arg, x, y };
          editor.previewEdit(drag.edit);
        }
        break;
      }
      case "marquee": {
        drag.end = p;
        const [x0, x1] = [Math.min(drag.start[0], p[0]), Math.max(drag.start[0], p[0])];
        const [y0, y1] = [Math.min(drag.start[1], p[1]), Math.max(drag.start[1], p[1])];
        const hits = new Set(drag.base);
        for (const probe of editor.probes) {
          const id = editor.selectableFor(probe.id);
          const b = probeBounds(probe);
          if (id === undefined || !b) continue;
          const [ax, ay] = editor.toPage(frameOf(probe), [b.x0, b.y1]);
          const [bx, by] = editor.toPage(frameOf(probe), [b.x1, b.y0]);
          if (ax <= x1 && bx >= x0 && ay <= y1 && by >= y0) hits.add(id);
        }
        editor.selection = [...hits];
        // Points are only picked up while they're on show.
        if (editor.showPoints) {
          const points = new Set(drag.basePoints);
          for (const [id, where] of placed) {
            const [px, py] = where?.page ?? [NaN, NaN];
            if (px >= x0 && px <= x1 && py >= y0 && py <= y1) points.add(id);
          }
          editor.pointSelection = [...points];
        }
        break;
      }
      case "rotate": {
        const { spin } = drag;
        // Whole degrees; ⇧ for 15° steps (which include level and upright), ⌘ for neither.
        let angle = wrapAngle(spin.angle + spin.sign * (pageAngle(spin.pivot, p) - drag.from));
        if (mods.angle) angle = wrapAngle(Math.round(angle / 15) * 15);
        else if (!mods.free) angle = Math.round(angle);
        drag.angle = angle;
        drag.edit = angle === spin.angle ? undefined : spin.edit(angle);
        if (drag.edit) editor.previewEdit(drag.edit);
        else editor.endDrag();
        break;
      }
      case "grow": {
        const { grow } = drag;
        const ratio = Math.hypot(p[0] - grow.pivot[0], p[1] - grow.pivot[1]) / (drag.from || 1);
        // Steps of 0.05, ⇧ quarters, ⌘ free.
        const step = mods.free ? 1e-3 : mods.angle ? 0.25 : 0.05;
        drag.factor = Math.max(step, Math.round((grow.factor * ratio) / step) * step);
        drag.edit = drag.factor === grow.factor ? undefined : grow.edit(Number(drag.factor.toFixed(4)));
        if (drag.edit) editor.previewEdit(drag.edit);
        else editor.endDrag();
        break;
      }
      case "sweep": {
        const next = sweepTo(drag.end, p, drag.sweep);
        drag.sweep = next.sweep;
        drag.angle = next.angle;
        drag.edit = next.edit;
        if (drag.edit) editor.previewEdit(drag.edit);
        else editor.endDrag();
        break;
      }
      case "reshape": {
        const { probe } = drag.reshape;
        drag.edit = drag.reshape.edit(snapPointGuided(frameOf(probe), probe.transform, p, editor.selected));
        if (drag.edit) editor.previewEdit(drag.edit);
        else editor.endDrag();
        break;
      }
      case "radius": {
        drag.r = radiusAt(drag.reach, p);
        drag.edit = { kind: "set-named", call: drag.reach.call, key: "radius", text: drag.reach.text(drag.r, mods.detach) };
        editor.previewEdit(drag.edit);
        break;
      }
      case "create": {
        if (mods.angle && isLineTool()) {
          const from = editor.toPage(drag.frame, transformPoint(drag.transform, drag.start));
          drag.endSnap = undefined;
          drag.end = angled(drag.frame, drag.transform, from, p).local;
          break;
        }
        // Polygons and arcs snap their radius and angle instead (see `polar`).
        if (isRadialTool()) {
          drag.end = pageToLocal(drag.frame, drag.transform, p);
          break;
        }
        drag.endSnap = findSnap(p, undefined, { anchors: isLineTool(), vertices: isLineTool(), at: insertAt() });
        if (drag.endSnap) guides = [];
        drag.end = drag.endSnap ? pageToLocal(drag.frame, drag.transform, drag.endSnap.point) : snapPointGuided(drag.frame, drag.transform, p, []);
        break;
      }
    }
  }

  /** Snaps a move so the selection's first literal coordinate lands on the grid. */
  function snapDelta(dx: number, dy: number): Point {
    if (!editor.snap || mods.free) return [dx, dy];
    const call = editor.callById.get(editor.selected[0]);
    const anchor = call?.args.find((a) => a.key === null && a.value.type === "coord")?.value;
    if (anchor?.type !== "coord") return [editor.snapValue(dx), editor.snapValue(dy)];
    return [editor.snapValue(anchor.x + dx) - anchor.x, editor.snapValue(anchor.y + dy) - anchor.y];
  }

  function onpointerup() {
    const d = drag;
    drag = undefined;
    guides = [];
    guideCache = undefined;
    if (!d) return;
    switch (d.kind) {
      case "move":
        if (d.moved && (d.delta[0] !== 0 || d.delta[1] !== 0)) {
          editor.endDrag({ kind: "move", calls: editor.selected, dx: d.delta[0], dy: d.delta[1], detach: d.detach });
        } else {
          editor.endDrag();
        }
        break;
      case "point":
        if (d.moved) editor.endDrag(d.edit);
        // A click (no drag) on one of several selected points picks just it.
        else if (d.group?.length) editor.pointSelection = [d.point];
        break;
      case "handle":
      case "rotate":
      case "radius":
      case "reshape":
      case "sweep":
      case "grow":
        editor.endDrag(d.edit);
        break;
      case "create":
        create(d);
        break;
    }
  }

  function ondblclick(e: MouseEvent) {
    // Double-clicking a shared point selects the shapes that use it.
    const grab = grabAt(pagePoint(e));
    const marker = grab && ("point" in grab ? grab.point : grab.handle.shared);
    if (marker !== undefined) {
      const users = (editor.pointUsers.get(marker) ?? []).map((id) => editor.selectableFor(id));
      editor.pointSelection = [];
      editor.selection = [...new Set(users.filter((id) => id !== undefined))];
      return;
    }
    // A shape's own handle (text's sits in its middle) counts as the shape.
    const onHandle = grab && "handle" in grab ? (editor.selectableFor(grab.handle.call) ?? grab.handle.call) : undefined;
    const hit = (e.target as Element).closest("[data-id]")?.getAttribute("data-id") ?? onHandle;
    if (hit === null || hit === undefined) {
      editor.scope = undefined;
      return;
    }
    const id = Number(hit);
    // Text (also turned or scaled) is edited where it is.
    const shape = editor.wrappedShape(id) ?? editor.callById.get(id);
    if (shape && startTextEdit(shape.id, pagePoint(e))) return;
    const hasChildren = editor.calls.some((c) => c.parent === id);
    if (hasChildren) {
      editor.scope = id;
      editor.selection = [];
    } else {
      void tick().then(() => editor.focusInspector?.());
    }
  }

  // --- Editing text in place ------------------------------------------------

  /**
   * Text being edited over its shape: the source of a `content(..)`'s
   * `[markup]` body or string argument. `at` (page) places the editor until
   * the shape has been drawn, as when the text tool has just made it.
   */
  type TextEdit = { call: number; arg: number; str: boolean; text: string; at: Point; select: boolean };
  let textEdit = $state<TextEdit>();
  let textArea = $state<HTMLTextAreaElement>();

  /** Starts editing a `content(..)`'s text; false when it has no `[..]` or string to edit. */
  function startTextEdit(id: number, at: Point, select = false): boolean {
    const call = editor.callById.get(id);
    if (!call || baseName(call.callee) !== "content" || call.in_loop) return false;
    const arg = call.args.findIndex((a) => a.key === null && (a.value.type === "content" || a.value.type === "str"));
    const value = call.args[arg]?.value;
    if (value?.type === "content") textEdit = { call: id, arg, str: false, text: editor.index.slice(value.inner.start, value.inner.end), at, select };
    else if (value?.type === "str") textEdit = { call: id, arg, str: true, text: value.value, at, select };
    else return false;
    return true;
  }

  function finishTextEdit(save: boolean) {
    const t = textEdit;
    const value = textArea?.value;
    textEdit = undefined;
    if (!t || !save || value === undefined || value === t.text) return;
    editor.edit({ kind: "set-arg-text", call: t.call, arg: t.arg, text: t.str ? JSON.stringify(value) : `[${value}]` });
  }

  /** Where the text editor sits, in viewport pixels: over the text once it's drawn. */
  const textBox = $derived.by(() => {
    if (!textEdit) return undefined;
    const probe = probeOf.get(textEdit.call);
    const b = probe && probeBounds(probe);
    const [a, c] = probe && b ? [editor.toPage(frameOf(probe), [b.x0, b.y1]), editor.toPage(frameOf(probe), [b.x1, b.y0])] : [textEdit.at, textEdit.at];
    const screen = (q: Point) => [editor.pan[0] + q[0] * editor.zoom, editor.pan[1] + q[1] * editor.zoom];
    const [[left, top], [right, bottom]] = [screen(a), screen(c)];
    return { left, top, width: right - left, height: bottom - top };
  });

  $effect(() => {
    if (!textArea || !textEdit) return;
    textArea.focus();
    if (textEdit.select) textArea.select();
    else textArea.setSelectionRange(textArea.value.length, textArea.value.length);
  });

  // --- Creating shapes -----------------------------------------------------

  function isLineTool() {
    return editor.tool === "line" || editor.tool === "arrow" || editor.tool === "brace";
  }

  /** Tools that build a path point by point: join (a line) and curve (a catmull). */
  function isJoinTool() {
    return editor.tool === "join" || editor.tool === "curve" || editor.tool === "angle";
  }

  /** Tools drawn out from a centre: the drag sets a radius and an angle. */
  function isRadialTool() {
    return editor.tool === "polygon" || editor.tool === "arc";
  }

  /** Sides of a polygon the polygon tool draws. */
  const POLYGON_SIDES = 6;

  /**
   * Radius and angle (degrees) of `p` around `c`, both local. They snap to
   * the grid step and to 15° unless ⌘ is held.
   */
  function polar(c: Point, p: Point): { r: number; deg: number } {
    let r = Math.hypot(p[0] - c[0], p[1] - c[1]);
    let deg = (Math.atan2(p[1] - c[1], p[0] - c[0]) * 180) / Math.PI;
    if (!mods.free) {
      r = editor.snapValue(r);
      deg = Math.round(deg / 15) * 15;
    }
    return { r, deg };
  }

  /** The point at `deg` and radius `r` around `c`. */
  function around(c: Point, r: number, deg: number): Point {
    const a = (deg * Math.PI) / 180;
    return [c[0] + r * Math.cos(a), c[1] + r * Math.sin(a)];
  }

  /** SVG path data, in page points, through local points. */
  function localPath(frame: Frame, transform: number[][] | undefined, pts: Point[], close = false): string {
    const page = pts.map((q) => editor.toPage(frame, transformPoint(transform, q)));
    return page.map((q, i) => `${i ? "L" : "M"}${q[0]},${q[1]}`).join(" ") + (close ? " Z" : "");
  }

  /** Points along an arc of radius `r` around `c`, `sweep` degrees from `from`. */
  function arcPoints(c: Point, r: number, from: number, sweep: number): Point[] {
    const n = Math.max(2, Math.ceil(Math.abs(sweep) / 5));
    return Array.from({ length: n + 1 }, (_, i) => around(c, r, from + (sweep * i) / n));
  }

  /** The circle an arc lies on and its radius out to `p`, while its radius is being set. */
  function radiusGuide(frame: Frame, transform: number[][] | undefined, c: Point, p: Point): string {
    const { r, deg } = polar(c, p);
    return `${localPath(frame, transform, arcPoints(c, r, deg, 360))} ${localPath(frame, transform, [c, around(c, r, deg)])}`;
  }

  // --- Arc tool ------------------------------------------------------------

  /**
   * A half-made arc. Dragging out from the centre (or clicking the centre and
   * then the start) sets its radius and start; moving then sweeps it either
   * way round, and a click finishes it.
   */
  type Arcing = {
    center: Point;
    centerSnap?: Snap;
    frame: Frame;
    transform?: number[][];
    /** Set once the start is placed. */
    r?: number;
    start?: number;
    /** Degrees from the start, negative for clockwise. */
    sweep: number;
    /** The pointer, local, while placing the start. */
    pointer?: Point;
  };
  let arcing = $state<Arcing>();

  function arcMove(a: Arcing, p: Point) {
    const local = pageToLocal(a.frame, a.transform, p);
    if (a.r === undefined || a.start === undefined) {
      a.pointer = local;
      return;
    }
    // Of the angles that reach the pointer, take the one nearest the current
    // sweep, so the arc follows the pointer round and past the start.
    let d = polar(a.center, local).deg - a.start;
    d += 360 * Math.round((a.sweep - d) / 360);
    a.sweep = Math.max(-360, Math.min(360, d));
  }

  function arcClick(a: Arcing, p: Point) {
    arcMove(a, p);
    if (a.r === undefined) {
      const { r, deg } = polar(a.center, pageToLocal(a.frame, a.transform, p));
      if (r > 1e-6) Object.assign(a, { r, start: deg, sweep: 0 });
      return;
    }
    if (Math.abs(a.sweep) < 1e-6 || a.start === undefined) return;
    const center = a.centerSnap?.ref ?? `(${num(a.center[0])}, ${num(a.center[1])})`;
    arcing = undefined;
    insertShape(`arc(${center}, start: ${num(a.start)}deg, stop: ${num(a.start + a.sweep)}deg, radius: ${num(a.r)}, anchor: "origin")`, [[0, a.centerSnap]]);
  }

  // Leaving the arc tool abandons a half-made arc.
  $effect(() => {
    if (editor.tool !== "arc") arcing = undefined;
  });

  function create(d: Extract<Drag, { kind: "create" }>) {
    const [x0, y0] = d.start;
    let [x1, y1] = d.end;
    const tiny = Math.hypot(x1 - x0, y1 - y0) < 1e-6;
    const pt = (x: number, y: number) => `(${num(x)}, ${num(y)})`;
    // Ends snapped to a named point are written by name.
    const startRef = d.startSnap?.ref;
    const endRef = tiny ? undefined : d.endSnap?.ref;
    const a = startRef ?? pt(x0, y0);
    let text: string;
    switch (editor.tool) {
      case "line":
      case "arrow":
        if (tiny) [x1, y1] = [x0 + 2, y0];
        text = `line(${a}, ${endRef ?? pt(x1, y1)}${editor.tool === "arrow" ? ', mark: (end: ">")' : ""})`;
        break;
      case "rect":
        if (tiny) [x1, y1] = [x0 + 2, y0 - 1];
        text =
          startRef || endRef
            ? `rect(${a}, ${endRef ?? pt(x1, y1)})`
            : `rect(${pt(Math.min(x0, x1), Math.min(y0, y1))}, ${pt(Math.max(x0, x1), Math.max(y0, y1))})`;
        break;
      case "circle": {
        const r = tiny ? 0.5 : Math.hypot(x1 - x0, y1 - y0);
        text = `circle(${a}, radius: ${num(r)})`;
        break;
      }
      case "polygon": {
        // The drag ends on the first corner, which sets the rotation.
        const { r, deg } = polar(d.start, d.end);
        const angle = r > 1e-6 ? deg : 0;
        text = `polygon(${a}, ${POLYGON_SIDES}, radius: ${num(r > 1e-6 ? r : 1)}${angle ? `, angle: ${num(angle)}deg` : ""})`;
        break;
      }
      case "arc": {
        // The drag places the centre and the start; the arc is swept next.
        const { r, deg } = polar(d.start, d.end);
        arcing = { center: d.start, centerSnap: d.startSnap, frame: d.frame, transform: d.transform, sweep: 0, ...(r > 1e-6 ? { r, start: deg } : {}) };
        return;
      }
      case "text":
        text = `content(${a}, [Text])`;
        break;
      case "brace":
        if (tiny) [x1, y1] = [x0 + 2, y0];
        text = `brace(${a}, ${endRef ?? pt(x1, y1)})`;
        break;
      default:
        return;
    }
    insertShape(
      text,
      [
        [0, d.startSnap],
        [1, tiny ? undefined : d.endSnap],
      ],
      editor.tool === "brace" ? "decorations" : undefined,
      editor.toPage(d.frame, transformPoint(d.transform, d.start)),
    );
  }

  /** Adds a new shape to the active canvas and selects it. `snaps` are where its point arguments were snapped. */
  function insertShape(text: string, snaps: [number, Snap | undefined][], module?: string, at?: Point) {
    const canvas = editor.activeCanvas ?? null;
    // A library's call (`decorations.brace`) also imports the library if needed.
    const steps: Parameters<Editor["chain"]>[0] = [module ? { kind: "insert-library", canvas, module, text } : { kind: "insert", canvas, text }];
    // Ends snapped to another shape's anchor connect to it (naming it if
    // needed); ends on a line's vertex share it.
    for (const [arg, snap] of snaps) {
      if (snap && snap.ref === undefined) steps.push(({ created, map }) => snapEdit(created[0], arg, snap, map));
    }
    if (editor.chain(steps)) {
      editor.tool = "select";
      // New text opens for typing where it was placed, its placeholder selected.
      if (text.startsWith("content") && at) startTextEdit(editor.selected[0], at, true);
    }
  }

  const preview = $derived.by(() => {
    if (arcing) {
      const { center, frame, transform, r, start, sweep, pointer } = arcing;
      if (r !== undefined && start !== undefined) {
        const arc = sweep ? ` ${localPath(frame, transform, arcPoints(center, r, start, sweep))}` : "";
        return localPath(frame, transform, [center, around(center, r, start)]) + arc;
      }
      if (pointer) return radiusGuide(frame, transform, center, pointer);
      const c = editor.toPage(frame, transformPoint(transform, center));
      return `M${c[0] - 3},${c[1]} h6 M${c[0]},${c[1] - 3} v6`;
    }
    if (drag?.kind !== "create") return undefined;
    const a = editor.toPage(drag.frame, transformPoint(drag.transform, drag.start));
    const b = editor.toPage(drag.frame, transformPoint(drag.transform, drag.end));
    switch (editor.tool) {
      case "line":
      case "arrow":
        return `M${a[0]},${a[1]} L${b[0]},${b[1]}`;
      case "rect":
        return `M${a[0]},${a[1]} H${b[0]} V${b[1]} H${a[0]} Z`;
      case "circle": {
        const r = Math.hypot(b[0] - a[0], b[1] - a[1]);
        return `M${a[0] - r},${a[1]} a${r},${r} 0 1,0 ${2 * r},0 a${r},${r} 0 1,0 ${-2 * r},0`;
      }
      case "polygon": {
        const c = drag.start;
        const { r, deg } = polar(c, drag.end);
        const corners = Array.from({ length: POLYGON_SIDES }, (_, i) => around(c, r, deg + (360 / POLYGON_SIDES) * i));
        return localPath(drag.frame, drag.transform, corners, true);
      }
      case "arc":
        return radiusGuide(drag.frame, drag.transform, drag.start, drag.end);
      default:
        return `M${a[0] - 3},${a[1]} h6 M${a[0]},${a[1] - 3} v6`;
    }
  });

  const activeSnap = $derived(
    drag?.kind === "handle" || drag?.kind === "point"
      ? drag.snap
      : drag?.kind === "create"
        ? (drag.endSnap ?? drag.startSnap)
        : toolHover?.snap,
  );

  /**
   * While drawing a line, the anchors of shapes near the pointer (or under
   * it), so you can see what an end could snap to before you get there.
   */
  const anchorHints = $derived.by(() => {
    if (!isLineTool() || !pointer || mods.free || (drag && drag.kind !== "create")) return [];
    const p: Point = [(pointer[0] - editor.pan[0]) / editor.zoom, (pointer[1] - editor.pan[1]) / editor.zoom];
    const reach = 40 / editor.zoom;
    const near = new Set<number>();
    if (editor.hovered !== undefined) near.add(editor.hovered);
    for (const t of snapTargets) if (Math.hypot(t.point[0] - p[0], t.point[1] - p[1]) < reach) near.add(t.target);
    return snapTargets.filter((t) => near.has(t.target)).map((t) => t.point);
  });

  // --- Grid ----------------------------------------------------------------

  /** The page-space area the grid covers: the page, or everything in view. */
  const gridArea = $derived.by(() => {
    if (!pageSize) return undefined;
    if (!editor.infinite) return { x0: 0, y0: 0, x1: pageSize.w, y1: pageSize.h };
    const [px, py] = editor.pan;
    return { x0: -px / editor.zoom, y0: -py / editor.zoom, x1: (width - px) / editor.zoom, y1: (height - py) / editor.zoom };
  });

  /**
   * Grid lines in screen pixels, snapped like the rulers' ticks so the two
   * line up exactly and stay sharp.
   */
  const grid = $derived.by(() => {
    if (!editor.showGrid || !gridArea || editor.activeCanvas === undefined) return undefined;
    const { x0, y0, x1, y1 } = gridArea;
    const frame = editor.frameFor(editor.activeCanvas);
    const step = visibleStep(editor.gridStep, frame.length * editor.zoom);
    const [cx0, cy1] = editor.toCanvas(frame, [x0, y0]);
    const [cx1, cy0] = editor.toCanvas(frame, [x1, y1]);
    const [sx0, sy0] = [editor.pan[0] + x0 * editor.zoom, editor.pan[1] + y0 * editor.zoom];
    const [sx1, sy1] = [editor.pan[0] + x1 * editor.zoom, editor.pan[1] + y1 * editor.zoom];
    let d = "";
    for (let i = Math.ceil(cx0 / step); i * step <= cx1; i++) {
      const [px] = editor.toPage(frame, [i * step, 0]);
      d += `M${crisp(editor.pan[0] + px * editor.zoom)},${sy0}V${sy1}`;
    }
    for (let i = Math.ceil(cy0 / step); i * step <= cy1; i++) {
      const [, py] = editor.toPage(frame, [0, i * step]);
      d += `M${sx0},${crisp(editor.pan[1] + py * editor.zoom)}H${sx1}`;
    }
    return d;
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Shift" || e.key === "Meta" || e.key === "Control" || e.key === "Alt") readMods(e);
    if (menu && e.key === "Escape") {
      e.stopImmediatePropagation();
      menu = undefined;
      return;
    }
    if (joining && !(e.target as HTMLElement).closest?.(".cm-editor") && !(e.target instanceof HTMLInputElement)) {
      if (e.key === "Enter") {
        e.preventDefault();
        finishJoin(joining, false);
        return;
      }
      if (e.key === "Escape") {
        joining = undefined;
        return;
      }
    }
    if (arcing && e.key === "Escape") {
      arcing = undefined;
      return;
    }
    if (e.key === " " && !(e.target instanceof HTMLInputElement) && !(e.target as HTMLElement).closest?.(".cm-editor")) {
      spaceHeld = true;
    }
  }
  function onkeyup(e: KeyboardEvent) {
    if (e.key === "Shift" || e.key === "Meta" || e.key === "Control" || e.key === "Alt") readMods(e);
    if (e.key === " ") spaceHeld = false;
  }

  const cursor = $derived(
    drag?.kind === "rotate"
      ? "grabbing"
      : drag?.kind === "pan" || spaceHeld || nearGrab || nearSpin || nearGrow || nearReach || nearReshape || nearArcEnd
      ? "grab"
      : editor.tool !== "select"
        ? "crosshair"
        : editor.hovered !== undefined
          ? "move"
          : "default",
  );
</script>

<svelte:window {onkeydown} {onkeyup} />

<div
  class="viewport"
  class:infinite={editor.infinite}
  bind:this={viewport}
  bind:clientWidth={width}
  bind:clientHeight={height}
  style:cursor
  {onwheel}
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  {ondblclick}
  {oncontextmenu}
  onpointerleave={() => {
    editor.hovered = undefined;
    nearGrab = undefined;
    pointer = undefined;
    if (!drag) toolHover = undefined;
  }}
  role="application"
  aria-label="Drawing canvas"
>
  {#if editor.svg && pageSize}
    <div
      class="page"
      class:stale={editor.hasErrors}
      style:width="{pageSize.w}px"
      style:height="{pageSize.h}px"
      style:transform="translate({editor.pan[0]}px, {editor.pan[1]}px) scale({editor.zoom})"
    >
      {@html editor.svg}
    </div>

    <svg class="overlay" width={width} height={height}>
      {#if grid}<path class="grid" d={grid} />{/if}
      <g transform="translate({editor.pan[0]} {editor.pan[1]}) scale({editor.zoom})">
        {#each editor.pageHeights.slice(0, -1) as _, i}
          {@const y = editor.pageHeights.slice(0, i + 1).reduce((a, b) => a + b, 0)}
          {#if editor.infinite && gridArea}
            <path class="page-break" d="M{gridArea.x0},{y}H{gridArea.x1}" />
          {:else}
            <rect class="page-gap" x="0" y={y - 0.5 / editor.zoom} width={pageSize.w} height={6 / editor.zoom} />
          {/if}
        {/each}

        {#each shapes as s, i (i)}
          {#if s.target !== undefined && editor.tool === "select"}
            <path class="hit" data-id={s.target} d={s.d} />
          {/if}
        {/each}

        {#each shapes as s, i (i)}
          {#if hoveredFamily.has(s.probe.id) && !selectedFamily.has(s.probe.id)}
            <path class="hovered" class:from-panel={editor.hoverSource === "panel"} d={s.d} />
          {/if}
        {/each}

        {#if instanceShape}
          {#if instanceShape.d}
            <path class="instance" d={instanceShape.d} />
          {:else if isVec(instanceShape.probe.anchors["default"])}
            {@const [ix, iy] = editor.toPage(frameOf(instanceShape.probe), instanceShape.probe.anchors["default"] as Vec3)}
            <circle class="instance" cx={ix} cy={iy} r={7 / editor.zoom} />
          {/if}
        {/if}

        <g transform="translate({moveShift[0]} {moveShift[1]})">
          {#each shapes as s, i (i)}
            {#if selectedFamily.has(s.probe.id)}
              <path class="selected" d={s.d} />
            {/if}
          {/each}
          {#if selectionBox}
            <rect
              class="selection-box"
              x={selectionBox.x0}
              y={selectionBox.y0}
              width={selectionBox.x1 - selectionBox.x0}
              height={selectionBox.y1 - selectionBox.y0}
            />
          {/if}
        </g>

        {#if growKnob && drag?.kind !== "grow"}
          <line class="spin-stem" x1={growKnob.stem[0]} y1={growKnob.stem[1]} x2={growKnob.knob[0]} y2={growKnob.knob[1]} />
          <circle class="handle spin" class:near={nearGrow} cx={growKnob.knob[0]} cy={growKnob.knob[1]} r={4.5 / editor.zoom} />
        {/if}
        {#if drag?.kind === "grow"}
          {@const pivot = drag.grow.pivot}
          <path class="spin-guide" d="M{pivot[0] - 5 / editor.zoom},{pivot[1]} h{10 / editor.zoom} M{pivot[0]},{pivot[1] - 5 / editor.zoom} v{10 / editor.zoom}" />
        {/if}
        {#if drag?.kind === "rotate"}
          {@const pivot = drag.spin.pivot}
          <path class="spin-guide" d="M{pivot[0] - 5 / editor.zoom},{pivot[1]} h{10 / editor.zoom} M{pivot[0]},{pivot[1] - 5 / editor.zoom} v{10 / editor.zoom}" />
        {:else if spinHandle}
          <line class="spin-stem" x1={spinHandle.stem[0]} y1={spinHandle.stem[1]} x2={spinHandle.knob[0]} y2={spinHandle.knob[1]} />
          <circle class="handle spin" class:near={nearSpin} cx={spinHandle.knob[0]} cy={spinHandle.knob[1]} r={4.5 / editor.zoom} />
        {/if}

        {#each reshapes as r, i (i)}
          <circle class="handle" class:edge={r.edge} cx={r.point[0]} cy={r.point[1]} r={(r.edge ? 3.5 : 4.5) / editor.zoom} />
        {/each}

        {#each arcEnds as k (k.end)}
          <circle class="handle spin" cx={k.point[0]} cy={k.point[1]} r={4 / editor.zoom} />
        {/each}

        {#each reaches as reach, i (i)}
          {@const r = 4 / editor.zoom}
          <rect class="handle" class:near={nearReach} x={reach.point[0] - r} y={reach.point[1] - r} width={2 * r} height={2 * r} />
        {/each}

        {#each guides as g, i (i)}
          {#if g.x !== undefined}
            <line class="guide" x1={g.x} y1={g.from} x2={g.x} y2={g.to} />
          {:else}
            <line class="guide" x1={g.from} y1={g.y} x2={g.to} y2={g.y} />
          {/if}
        {/each}

        {#each markers as m (m.id)}
          <g class="point" class:hovered={editor.hoveredPoint === m.id} class:picked={m.selected} data-point={m.id}>
            <circle cx={m.page[0]} cy={m.page[1]} r={4 / editor.zoom} />
            <text x={m.page[0] + 6 / editor.zoom} y={m.page[1] - 6 / editor.zoom} font-size={11 / editor.zoom}>{pointLabel(m.id)}</text>
          </g>
        {/each}

        {#each handles as h (h.arg)}
          {@const r = 4.5 / editor.zoom}
          {#if h.shared !== undefined}
            <g class="point shared" class:hovered={editor.hoveredPoint === h.shared} data-shared={h.shared}>
              <circle class="handle" data-handle="{h.call}:{h.arg}" cx={h.point[0]} cy={h.point[1]} r={r + 0.5 / editor.zoom} />
              <text x={h.point[0] + 7 / editor.zoom} y={h.point[1] - 7 / editor.zoom} font-size={11 / editor.zoom}>{pointLabel(h.shared)}</text>
            </g>
          {:else if h.linked}
            <rect
              class="handle linked"
              class:near={nearHandle === h.arg}
              data-handle="{h.call}:{h.arg}"
              x={h.point[0] - r}
              y={h.point[1] - r}
              width={2 * r}
              height={2 * r}
              transform="rotate(45 {h.point[0]} {h.point[1]})"
            />
          {:else}
            <circle class="handle" class:near={nearHandle === h.arg} data-handle="{h.call}:{h.arg}" cx={h.point[0]} cy={h.point[1]} {r} />
          {/if}
        {/each}

        {#if drag?.kind === "marquee"}
          <rect
            class="marquee"
            x={Math.min(drag.start[0], drag.end[0])}
            y={Math.min(drag.start[1], drag.end[1])}
            width={Math.abs(drag.end[0] - drag.start[0])}
            height={Math.abs(drag.end[1] - drag.start[1])}
          />
        {/if}

        {#if preview}<path class="preview" d={preview} />{/if}
        {#if joinPreview}<path class="preview" d={joinPreview} />{/if}
        {#if joining}
          <!-- The white step is where clicking closes the path. -->
          {@const closer = joining.extend?.other ?? joining.pages[0]}
          {#each joining.pages as q, i (i)}
            <circle class="join-step" cx={q[0]} cy={q[1]} r={3 / editor.zoom} />
          {/each}
          {#if closer}
            <circle class="join-step first" cx={closer[0]} cy={closer[1]} r={4.5 / editor.zoom} />
          {/if}
        {/if}
        {#if toolHover && !toolHover.snap}
          <path
            class="tool-ghost"
            d="M{toolHover.page[0] - 6 / editor.zoom},{toolHover.page[1]}h{12 / editor.zoom}M{toolHover.page[0]},{toolHover.page[1] - 6 / editor.zoom}v{12 / editor.zoom}"
          />
        {/if}

        {#each anchorHints as q, i (i)}
          <circle class="anchor-hint" cx={q[0]} cy={q[1]} r={2.5 / editor.zoom} />
        {/each}
        {#if activeSnap}
          <circle
            class="snap"
            class:named={(activeSnap.named !== undefined || activeSnap.vertex !== undefined) && !(joining && mods.detach)}
            cx={activeSnap.point[0]}
            cy={activeSnap.point[1]}
            r={6 / editor.zoom}
          />
        {/if}
      </g>
    </svg>
    {#if editor.showRulers}
      <Rulers
        {editor}
        {width}
        {height}
        cursor={pointer}
        band={selectionBox && {
          x0: selectionBox.x0 + moveShift[0],
          x1: selectionBox.x1 + moveShift[0],
          y0: selectionBox.y0 + moveShift[1],
          y1: selectionBox.y1 + moveShift[1],
        }}
      />
    {/if}
    {#if menu}
      {@const rows = menu.sections.reduce((n, items) => n + items.length, 0)}
      <div
        class="menu"
        role="menu"
        tabindex="-1"
        style:left="{Math.min(menu.at[0], width - 230)}px"
        style:top="{Math.max(0, Math.min(menu.at[1], height - 8 - 30 * rows - 9 * menu.sections.length))}px"
        onpointerdown={(e) => e.stopPropagation()}
        oncontextmenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
        }}
      >
        {#each menu.sections as items, i (i)}
          {#if i > 0}<hr />{/if}
          {#each items as item (item.label)}
            <button
              role="menuitem"
              onclick={() => {
                menu = undefined;
                item.run();
              }}
              ><span>{item.label}</span>{#if item.keys}<kbd>{item.keys}</kbd>{/if}</button
            >
          {/each}
        {/each}
      </div>
    {/if}
    {#if textEdit && textBox}
      <!-- Typst's default 11pt text, at the canvas's zoom. -->
      <textarea
        class="text-edit"
        bind:this={textArea}
        value={textEdit.text}
        style:left="{textBox.left - 4}px"
        style:top="{textBox.top - 3}px"
        style:min-width="{Math.max(textBox.width + 8, 80)}px"
        style:min-height="{textBox.height + 6}px"
        style:font-size="{11 * editor.zoom}px"
        rows={textEdit.text.split("\n").length}
        spellcheck="false"
        onpointerdown={(e) => e.stopPropagation()}
        onblur={() => finishTextEdit(true)}
        onkeydown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            finishTextEdit(true);
          } else if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            finishTextEdit(false);
          }
        }}
      ></textarea>
    {/if}
    {#if continuable}
      <div class="hint">Click an end to continue this line</div>
    {:else if editor.tool === "angle" && (joining || toolHover)}
      <div class="hint">Click the corner, then a point on each side · Esc cancels</div>
    {:else if joining?.extend}
      <div class="hint">Click the other end to close · Enter to finish · ⇧ 15° · {isMac ? "⌥" : "Alt"} don't share · {isMac ? "⌘" : "Ctrl"} no snapping</div>
    {:else if joining}
      <div class="hint">Click the first point to close · Enter to finish · ⇧ 15° · {isMac ? "⌥" : "Alt"} don't share · {isMac ? "⌘" : "Ctrl"} no snapping</div>
    {:else if drag?.kind === "grow"}
      <div class="hint">×{num(drag.factor)} · ⇧ quarter steps · {isMac ? "⌘" : "Ctrl"} free</div>
    {:else if drag?.kind === "sweep"}
      <div class="hint">{drag.end.end} {num(drag.angle ?? drag.end[drag.end.end])}° · ⇧ 15° steps · {isMac ? "⌘" : "Ctrl"} no rounding</div>
    {:else if drag?.kind === "radius"}
      <div class="hint">radius {num(drag.r)}{drag.reach.stretchy ? ` · ${isMac ? "⌥" : "Alt"} ellipse` : ""} · {isMac ? "⌘" : "Ctrl"} no snapping</div>
    {:else if drag?.kind === "rotate"}
      <div class="hint">{num(drag.angle)}° · ⇧ 15° steps</div>
    {:else if drag?.kind === "create" && isLineTool()}
      <div class="hint">⇧ 15° steps · {isMac ? "⌘" : "Ctrl"} no snapping</div>
    {:else if drag?.kind === "create" && editor.tool === "polygon"}
      <div class="hint">Drag to a corner · {isMac ? "⌘" : "Ctrl"} no snapping</div>
    {:else if (drag?.kind === "create" && editor.tool === "arc") || (arcing && arcing.r === undefined)}
      <div class="hint">{drag ? "Drag" : "Click"} where the arc starts · Esc cancels</div>
    {:else if arcing}
      <div class="hint">Move to sweep · click to finish · Esc cancels</div>
    {/if}
    {#if sharing}
      <div class="hint">{detaching ? "Detaching from the shared point" : "Moving shared points · hold ⌥ to detach"}</div>
    {/if}
  {:else}
    <div class="empty">
      {editor.hasErrors
        ? "Fix the errors to see the drawing."
        : editor.status.kind === "done"
          ? "Nothing to show."
          : editor.status.kind === "fetching"
            ? `Fetching ${editor.status.packages.join(", ")}…`
            : "Compiling…"}
    </div>
  {/if}
</div>

<style>
  .viewport {
    position: relative;
    overflow: hidden;
    background: var(--canvas-bg);
    touch-action: none;
    user-select: none;
    height: 100%;
  }
  .page {
    position: absolute;
    top: 0;
    left: 0;
    transform-origin: 0 0;
    background: white;
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.12), 0 0 0 1px rgba(0, 0, 0, 0.04);
  }
  .page.stale {
    opacity: 0.45;
  }
  /* Infinite: the whole viewport is the sheet, and nothing is clipped at the page edge. */
  .viewport.infinite {
    background: white;
  }
  .infinite .page {
    box-shadow: none;
  }
  .infinite .page > :global(svg) {
    overflow: visible;
  }
  .page > :global(svg) {
    display: block;
    width: 100%;
    height: 100%;
  }
  .overlay {
    position: absolute;
    inset: 0;
    overflow: visible;
  }
  .overlay path,
  .overlay rect,
  .overlay circle {
    vector-effect: non-scaling-stroke;
  }
  .grid {
    stroke: var(--grid);
    stroke-width: 1;
    fill: none;
    pointer-events: none;
  }
  .page-break {
    stroke: var(--muted);
    stroke-width: 1;
    stroke-dasharray: 6 4;
    opacity: 0.5;
    pointer-events: none;
  }
  .page-gap {
    fill: var(--canvas-bg);
    pointer-events: none;
  }
  .hit {
    fill: transparent;
    stroke: transparent;
    stroke-width: 10;
  }
  .hovered {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1;
    pointer-events: none;
  }
  /* Pointed at from the outline: as loud as a pinned loop repetition. */
  .hovered.from-panel {
    fill: color-mix(in srgb, var(--snap) 12%, transparent);
    stroke: var(--snap);
    stroke-width: 2.5;
  }
  .selected {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    pointer-events: none;
  }
  .instance {
    fill: color-mix(in srgb, var(--snap) 12%, transparent);
    stroke: var(--snap);
    stroke-width: 2.5;
    pointer-events: none;
  }
  .text-edit {
    position: absolute;
    z-index: 2;
    padding: 2px 3px;
    font-family: "Libertinus Serif", "New Computer Modern", serif;
    line-height: 1.2;
    color: var(--text);
    background: var(--bg);
    border: 1.5px solid var(--accent);
    border-radius: 3px;
    outline: none;
    resize: none;
    field-sizing: content;
  }
  .guide {
    stroke: var(--snap);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
    pointer-events: none;
  }
  .spin-stem {
    stroke: var(--accent);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
    pointer-events: none;
  }
  .spin-guide {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    pointer-events: none;
  }
  .selection-box {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1;
    stroke-dasharray: 4 3;
    pointer-events: none;
  }
  .handle {
    fill: white;
    stroke: var(--accent);
    stroke-width: 1.5;
    cursor: grab;
  }
  .handle.linked {
    fill: var(--accent);
  }
  .handle.near {
    stroke-width: 3;
  }
  .point circle {
    fill: var(--point);
    stroke: white;
    stroke-width: 1.5;
    cursor: grab;
  }
  .point text {
    fill: var(--point);
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-weight: 600;
    paint-order: stroke;
    stroke: white;
    stroke-width: 3px;
    pointer-events: none;
  }
  .point.hovered circle {
    stroke: var(--point);
    stroke-width: 3;
  }
  /* Not `.selected`: that's the selected shape's outline, which ignores the pointer. */
  .point.picked circle {
    stroke: var(--accent);
    stroke-width: 3;
  }
  .hint {
    position: absolute;
    bottom: 14px;
    left: 50%;
    transform: translateX(-50%);
    padding: 4px 10px;
    background: var(--text);
    color: var(--bg);
    font-size: 12px;
    pointer-events: none;
    /* Wraps on a narrow canvas instead of running off both sides. */
    width: max-content;
    max-width: calc(100% - 24px);
    text-align: center;
    border-radius: 12px;
  }
  .menu {
    position: absolute;
    z-index: 2;
    display: flex;
    flex-direction: column;
    min-width: 180px;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.18);
  }
  .menu hr {
    margin: 4px 6px;
    border: none;
    border-top: 1px solid var(--border);
  }
  .menu button {
    display: flex;
    justify-content: space-between;
    gap: 24px;
    padding: 6px 10px;
    border: none;
    border-radius: 5px;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: default;
  }
  .menu kbd {
    color: var(--muted);
    font: inherit;
  }
  .menu button:hover kbd,
  .menu button:focus-visible kbd {
    color: inherit;
    opacity: 0.8;
  }
  .menu button:hover,
  .menu button:focus-visible {
    background: var(--accent);
    color: white;
    outline: none;
  }
  .marquee {
    fill: color-mix(in srgb, var(--accent) 8%, transparent);
    stroke: var(--accent);
    stroke-width: 1;
    pointer-events: none;
  }
  .preview {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    stroke-dasharray: 5 3;
    pointer-events: none;
  }
  .snap {
    fill: none;
    stroke: var(--snap);
    stroke-width: 2;
    pointer-events: none;
  }
  .anchor-hint {
    fill: var(--snap);
    opacity: 0.6;
    pointer-events: none;
  }
  .join-step {
    fill: var(--accent);
    stroke: white;
    stroke-width: 1.5;
    pointer-events: none;
  }
  .join-step.first {
    fill: white;
    stroke: var(--accent);
  }
  .tool-ghost {
    stroke: var(--point);
    stroke-width: 1.5;
    pointer-events: none;
  }
  .snap.named {
    stroke: var(--point);
    stroke-width: 2.5;
  }
  .empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--muted);
  }
</style>
