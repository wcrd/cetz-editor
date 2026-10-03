<script lang="ts">
  // The drawing surface: the Typst render, with an interaction overlay built
  // from the probe geometry. Works in page points (the SVG's viewBox units);
  // `zoom` maps points to screen pixels.
  import { tick, untrack } from "svelte";
  import type { Editor, Frame } from "./editor.svelte";
  import { isVec, pathData, probeBounds, transformPoint, untransformDelta, type Probe, type Vec3 } from "./probe";
  import { baseName, type Call, type Edit } from "./scene";
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
    | { kind: "move"; start: Point; moved: boolean; delta: Point; detach: boolean }
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
    const call = editor.callById.get(editor.selected[0]);
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

  /** Anchors you can snap to: top-level shapes outside loops. */
  const snapTargets = $derived(
    editor.probes.flatMap((probe) => {
      const call = editor.callById.get(probe.id);
      if (!call || call.parent !== null || call.in_loop || probe.drawables.length === 0) return [];
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

  /**
   * The nearest snap target within reach. `exclude` skips anchors and
   * vertices of those shapes; `points: false` skips named points, `anchors: false` shape
   * anchors, and `vertices` adds lines' literal vertices. Named points win
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
    }: { points?: boolean; anchors?: boolean; vertices?: boolean; excludePoint?: number } = {},
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
    const shadow = 4 / editor.zoom;
    const corners: Snap[] = [];
    const corner = (t: Snap, weight: number) => {
      if (Math.hypot(t.point[0] - p[0], t.point[1] - p[1]) < radius + shadow) corners.push(t);
      consider(t, weight);
    };
    if (points) for (const t of pointTargets) if (t.named !== excludePoint) corner(t, 0.75);
    if (vertices) for (const t of vertexTargets) if (!excluded(t.vertex.call)) corner(t, 0.85);
    if (anchors) {
      const shadowed = (q: Point) => corners.some((c) => Math.hypot(c.point[0] - q[0], c.point[1] - q[1]) < shadow);
      for (const t of snapTargets) {
        if (excluded(t.target)) continue;
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
   * A line whose vertices can be edited: a plain `line(..)` outside loops.
   * `verts` are its positional arguments' indices.
   */
  function editablePath(id: number) {
    const call = editor.callById.get(id);
    const probe = probeOf.get(id);
    if (!call || !probe || call.in_loop || baseName(call.callee) !== "line") return undefined;
    const verts = call.args.flatMap((a, i) => (a.key === null ? [i] : []));
    const closed = call.args.some((a) => a.key === "close" && a.text.trim() === "true");
    return { call, probe, verts, closed };
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
    editor.tool = "join";
    const extend = { call: id, start, from, other, frame: frameOf(path.probe), transform: path.probe.transform };
    joining = { refs: [], pages: [], links: [], extend };
  }

  /** The open selected line the join tool would continue, before its first click. */
  const continuable = $derived.by(() => {
    if (editor.tool !== "join" || joining || editor.selected.length !== 1) return undefined;
    const path = editablePath(editor.selected[0]);
    return path && !path.closed && path.verts.length >= 2 ? path : undefined;
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

  /** Inserts a vertex on the line's segment nearest `p`, on the grid when snapping. */
  function addVertexAt(id: number, p: Point) {
    const path = editablePath(id);
    if (!path) return;
    const pages = path.verts.map((arg) => argHandle(path.call, path.probe, arg)?.point);
    let best: { at: number; q: Point; dist: number } | undefined;
    const count = path.closed ? pages.length : pages.length - 1;
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

  type MenuItem = { label: string; run: () => void };
  /** The right-click menu, at screen pixels in the viewport. */
  let menu = $state<{ at: Point; items: MenuItem[] }>();

  function oncontextmenu(e: MouseEvent) {
    if (joining || drag || editor.tool !== "select") return;
    const p = pagePoint(e);
    const items: MenuItem[] = [];
    const grab = grabAt(p);
    const hit = (e.target as Element).closest("[data-id]")?.getAttribute("data-id");
    if (grab && "handle" in grab) {
      const { call, arg } = grab.handle;
      const path = editablePath(call);
      if (path) {
        const end = path.verts.indexOf(arg) === 0 ? "start" : arg === path.verts[path.verts.length - 1] ? "end" : undefined;
        if (end && !path.closed) items.push({ label: "Continue line from here", run: () => continueLine(call, end === "start") });
        if (path.verts.length > (path.closed ? 3 : 2)) {
          items.push({ label: "Remove point", run: () => editor.edit({ kind: "remove-arg", call, arg, keep: path.closed ? 3 : 2 }) });
        }
      }
    } else if (hit !== null && hit !== undefined && editablePath(Number(hit))) {
      const id = Number(hit);
      const path = editablePath(id)!;
      editor.selection = [id];
      editor.pointSelection = [];
      items.push({ label: "Add point here", run: () => addVertexAt(id, p) });
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
    }
    if (items.length === 0) return;
    e.preventDefault();
    const r = viewport.getBoundingClientRect();
    menu = { at: [e.clientX - r.left, e.clientY - r.top], items };
  }

  // Leaving the join tool abandons a half-made path.
  $effect(() => {
    if (editor.tool !== "join") joining = undefined;
    if (editor.tool !== "point" && editor.tool !== "join") toolHover = undefined;
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
    const snap = join ? findSnap(p, extend?.call, { vertices: true }) : undefined;
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
  }

  function finishJoin(path: Joining, closed: boolean) {
    joining = undefined;
    if (path.extend) return finishExtend(path, path.extend, closed);
    if (path.refs.length < 2) return;
    const text = `line(${path.refs.join(", ")}${closed ? ", close: true" : ""})`;
    const steps: Parameters<Editor["chain"]>[0] = [{ kind: "insert", canvas: editor.activeCanvas ?? null, text }];
    for (const { arg, snap } of path.links) steps.push(({ created, map }) => snapEdit(created[0], arg, snap, map));
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
    const pages = [...from, ...joining.pages, ...(toolHover ? [toolHover.page] : [])];
    if (pages.length < 2) return undefined;
    return pages.map((q, i) => `${i ? "L" : "M"}${q[0]},${q[1]}`).join(" ");
  });

  function onpointerdown(e: PointerEvent) {
    menu = undefined;
    if (e.button === 2) return;
    viewport.setPointerCapture(e.pointerId);
    const p = pagePoint(e);
    readMods(e);

    if (e.button === 0 && !spaceHeld && editor.tool === "point") {
      placePoint(p);
      return;
    }
    if (e.button === 0 && !spaceHeld && editor.tool === "join") {
      joinAt(p, e.detail >= 2);
      return;
    }

    if (e.button === 1 || spaceHeld) {
      drag = { kind: "pan", start: [e.clientX, e.clientY], pan: [...editor.pan] };
      return;
    }

    if (editor.tool !== "select") {
      const { frame, transform } = creationFrame();
      const startSnap = findSnap(p, undefined, { anchors: isLineTool(), vertices: isLineTool() });
      const start = startSnap ? pageToLocal(frame, transform, startSnap.point) : snapPoint(frame, transform, p);
      drag = { kind: "create", start, end: start, startSnap, frame, transform };
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
      if (editor.selected.includes(id)) drag = { kind: "move", start: p, moved: false, delta: [0, 0], detach: e.altKey };
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
    if (!drag && (editor.tool === "point" || editor.tool === "join")) {
      toolHover = toolTarget(p, editor.tool === "join");
      return;
    }
    if (!drag) {
      // A point in reach takes the pointer from the shape under it.
      nearGrab = editor.tool === "select" && !spaceHeld ? grabAt(p) : undefined;
      const hit = nearGrab ? null : (e.target as Element).closest("[data-id]")?.getAttribute("data-id");
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
          const snap = findSnap(p, call, { vertices: true });
          drag.snap = snap;
          const edit = snap && snapEdit(call, arg, snap);
          if (edit) {
            drag.edit = edit;
          } else {
            const [x, y] = snapPoint(frameOf(probe), probe.transform, p);
            drag.edit = { kind: "set-coord", call, arg, x, y };
          }
        } else {
          // Move the shared point itself; snap it onto other points, or anchors of shapes that don't use it.
          const snap = findSnap(p, new Set(editor.pointUsers.get(drag.point) ?? []), { excludePoint: drag.point });
          drag.snap = snap;
          const [x, y] = snap ? pageToLocal(drag.frame, drag.transform, snap.point) : snapPoint(drag.frame, drag.transform, p);
          const own = editor.pointById.get(drag.point);
          drag.edit =
            drag.group?.length && own
              ? { kind: "move-points", points: [drag.point, ...drag.group], dx: x - own.x, dy: y - own.y }
              : { kind: "set-point", point: drag.point, x, y };
        }
        editor.previewEdit(drag.edit);
        break;
      }
      case "handle": {
        const snap = findSnap(p, drag.call, { vertices: true });
        drag.snap = snap;
        const edit = snap && snapEdit(drag.call, drag.arg, snap);
        if (edit) {
          drag.edit = edit;
        } else {
          const [x, y] = snapPoint(frameOf(drag.probe), drag.probe.transform, p);
          drag.edit = { kind: "set-coord", call: drag.call, arg: drag.arg, x, y };
        }
        editor.previewEdit(drag.edit);
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
      case "create": {
        if (mods.angle && isLineTool()) {
          const from = editor.toPage(drag.frame, transformPoint(drag.transform, drag.start));
          drag.endSnap = undefined;
          drag.end = angled(drag.frame, drag.transform, from, p).local;
          break;
        }
        drag.endSnap = findSnap(p, undefined, { anchors: isLineTool(), vertices: isLineTool() });
        drag.end = drag.endSnap ? pageToLocal(drag.frame, drag.transform, drag.endSnap.point) : snapPoint(drag.frame, drag.transform, p);
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
    const hit = (e.target as Element).closest("[data-id]")?.getAttribute("data-id");
    if (!hit) {
      editor.scope = undefined;
      return;
    }
    const id = Number(hit);
    const hasChildren = editor.calls.some((c) => c.parent === id);
    if (hasChildren) {
      editor.scope = id;
      editor.selection = [];
    } else {
      void tick().then(() => editor.focusInspector?.());
    }
  }

  // --- Creating shapes -----------------------------------------------------

  function isLineTool() {
    return editor.tool === "line" || editor.tool === "arrow";
  }

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
      case "text":
        text = `content(${a}, [Text])`;
        break;
      default:
        return;
    }
    const steps: Parameters<Editor["chain"]>[0] = [{ kind: "insert", canvas: editor.activeCanvas ?? null, text }];
    // Ends snapped to another shape's anchor connect to it (naming it if
    // needed); ends on a line's vertex share it.
    for (const [arg, snap] of [[0, d.startSnap], [1, tiny ? undefined : d.endSnap]] as const) {
      if (snap && snap.ref === undefined) steps.push(({ created, map }) => snapEdit(created[0], arg, snap, map));
    }
    if (editor.chain(steps)) {
      editor.tool = "select";
      // The inspector mounts once the new shape is selected.
      if (text.startsWith("content")) void tick().then(() => editor.focusInspector?.());
    }
  }

  const preview = $derived.by(() => {
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
    if (e.key === " " && !(e.target instanceof HTMLInputElement) && !(e.target as HTMLElement).closest?.(".cm-editor")) {
      spaceHeld = true;
    }
  }
  function onkeyup(e: KeyboardEvent) {
    if (e.key === "Shift" || e.key === "Meta" || e.key === "Control" || e.key === "Alt") readMods(e);
    if (e.key === " ") spaceHeld = false;
  }

  const cursor = $derived(
    drag?.kind === "pan" || spaceHeld || nearGrab
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
      <div
        class="menu"
        role="menu"
        tabindex="-1"
        style:left="{Math.min(menu.at[0], width - 190)}px"
        style:top="{Math.min(menu.at[1], height - 8 - 32 * menu.items.length)}px"
        onpointerdown={(e) => e.stopPropagation()}
        oncontextmenu={(e) => e.preventDefault()}
      >
        {#each menu.items as item (item.label)}
          <button
            role="menuitem"
            onclick={() => {
              menu = undefined;
              item.run();
            }}>{item.label}</button
          >
        {/each}
      </div>
    {/if}
    {#if continuable}
      <div class="hint">Click an end to continue this line</div>
    {:else if joining?.extend}
      <div class="hint">Click the other end to close · Enter to finish · ⇧ 15° · {isMac ? "⌥" : "Alt"} don't share · {isMac ? "⌘" : "Ctrl"} no snapping</div>
    {:else if joining}
      <div class="hint">Click the first point to close · Enter to finish · ⇧ 15° · {isMac ? "⌥" : "Alt"} don't share · {isMac ? "⌘" : "Ctrl"} no snapping</div>
    {:else if drag?.kind === "create" && isLineTool()}
      <div class="hint">⇧ 15° steps · {isMac ? "⌘" : "Ctrl"} no snapping</div>
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
    border-radius: 999px;
    background: var(--text);
    color: var(--bg);
    font-size: 12px;
    pointer-events: none;
    white-space: nowrap;
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
  .menu button {
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
