<script lang="ts">
  // The drawing surface: the Typst render, with an interaction overlay built
  // from the probe geometry. Works in page points (the SVG's viewBox units);
  // `zoom` maps points to screen pixels.
  import { untrack } from "svelte";
  import type { Editor, Frame } from "./editor.svelte";
  import { isVec, pathData, probeBounds, transformPoint, untransformDelta, type Probe } from "./probe";
  import type { Call, Edit } from "./scene";
  import { num } from "./format";

  let { editor }: { editor: Editor } = $props();

  let viewport: HTMLDivElement;
  let width = $state(0);
  let height = $state(0);
  let spaceHeld = $state(false);

  type Point = [number, number];
  type Snap = { target: number; anchor: string; point: Point };
  type Drag =
    | { kind: "pan"; start: Point; pan: Point }
    | { kind: "move"; start: Point; moved: boolean; delta: Point }
    | { kind: "handle"; call: number; arg: number; probe: Probe; snap?: Snap; edit?: Edit }
    | { kind: "marquee"; start: Point; end: Point; base: number[] }
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

  function zoomAt(factor: number, [sx, sy]: Point) {
    const z = Math.min(32, Math.max(0.1, editor.zoom * factor));
    const k = z / editor.zoom;
    editor.pan = [sx - (sx - editor.pan[0]) * k, sy - (sy - editor.pan[1]) * k];
    editor.zoom = z;
  }

  let fitted = false;
  $effect(() => {
    if (pageSize && width && height && !fitted) {
      fitted = true;
      fit();
    }
  });
  $effect(() => {
    editor.viewport = { fit, zoomBy: (f: number) => zoomAt(f, [width / 2, height / 2]) };
    return () => (editor.viewport = undefined);
  });
  // Refit when a document is loaded.
  $effect(() => {
    void editor.loads;
    fitted = false;
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

  function pagePoint(e: PointerEvent): Point {
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

  const shapes = $derived(
    editor.probes.map((probe) => ({
      probe,
      d: probe.drawables.map((d) => pathData(probe, d)).join(" "),
      target: editor.selectableFor(probe.id),
    })),
  );

  const selectedFamily = $derived(new Set(editor.selected.flatMap((id) => [...editor.family(id)])));
  const hoveredFamily = $derived(editor.hovered === undefined ? new Set<number>() : editor.family(editor.hovered));

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

  type Handle = { call: number; arg: number; point: Point; linked: boolean };

  /** Draggable points of a single selected call: its coordinate arguments. */
  const handles = $derived.by((): Handle[] => {
    if (editor.selected.length !== 1 || drag?.kind === "move") return [];
    const call = editor.callById.get(editor.selected[0]);
    const probe = call && probeOf.get(call.id);
    if (!call || !probe || call.in_loop) return [];
    const out: Handle[] = [];
    call.args.forEach((arg, i) => {
      if (arg.key !== null) return;
      if (arg.value.type === "coord") {
        out.push({ call: call.id, arg: i, point: localToPage(probe, [arg.value.x, arg.value.y]), linked: false });
      } else if (arg.value.type === "str") {
        const point = anchorPoint(arg.value.value);
        if (point) out.push({ call: call.id, arg: i, point, linked: true });
      }
    });
    return out;
  });

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

  function findSnap(p: Point, exclude?: number): Snap | undefined {
    const radius = 8 / editor.zoom;
    let best: Snap | undefined;
    let bestDist = radius;
    for (const t of snapTargets) {
      if (t.target === exclude) continue;
      const dist = Math.hypot(t.point[0] - p[0], t.point[1] - p[1]);
      if (dist < bestDist) {
        best = t;
        bestDist = dist;
      }
    }
    return best;
  }

  // --- Pointer interaction -------------------------------------------------

  function snapPoint(frame: Frame, transform: number[][] | undefined, p: Point): Point {
    const [x, y] = pageToLocal(frame, transform, p);
    return [editor.snapValue(x), editor.snapValue(y)];
  }

  /** The frame and transform new shapes are drawn in: the end of the active canvas. */
  function creationFrame(): { frame: Frame; transform?: number[][] } {
    const canvas = editor.activeCanvas;
    const last = [...editor.probes].reverse().find((p) => editor.canvasOfCall.get(p.id) === canvas);
    return { frame: editor.frameFor(canvas), transform: last?.transform };
  }

  function onpointerdown(e: PointerEvent) {
    if (e.button === 2) return;
    viewport.setPointerCapture(e.pointerId);
    const p = pagePoint(e);

    if (e.button === 1 || spaceHeld) {
      drag = { kind: "pan", start: [e.clientX, e.clientY], pan: [...editor.pan] };
      return;
    }

    if (editor.tool !== "select") {
      const { frame, transform } = creationFrame();
      const startSnap = isLineTool() ? findSnap(p) : undefined;
      const start = startSnap ? pageToLocal(frame, transform, startSnap.point) : snapPoint(frame, transform, p);
      drag = { kind: "create", start, end: start, startSnap, frame, transform };
      return;
    }

    const target = e.target as Element;
    const handle = target.closest("[data-handle]")?.getAttribute("data-handle");
    if (handle) {
      const [call, arg] = handle.split(":").map(Number);
      const probe = probeOf.get(call);
      if (probe) drag = { kind: "handle", call, arg, probe };
      return;
    }

    const hit = target.closest("[data-id]")?.getAttribute("data-id");
    if (hit !== null && hit !== undefined) {
      const id = Number(hit);
      if (e.shiftKey) {
        editor.selection = editor.selected.includes(id) ? editor.selected.filter((s) => s !== id) : [...editor.selected, id];
      } else if (!editor.selected.includes(id)) {
        editor.selection = [id];
      }
      if (editor.selected.includes(id)) drag = { kind: "move", start: p, moved: false, delta: [0, 0] };
      return;
    }

    const base = e.shiftKey ? [...editor.selected] : [];
    if (!e.shiftKey) editor.selection = [];
    drag = { kind: "marquee", start: p, end: p, base };
  }

  function onpointermove(e: PointerEvent) {
    const p = pagePoint(e);
    if (!drag) {
      const hit = (e.target as Element).closest("[data-id]")?.getAttribute("data-id");
      editor.hovered = hit ? Number(hit) : undefined;
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
        editor.dragMove(...drag.delta);
        break;
      }
      case "handle": {
        const snap = findSnap(p, drag.call);
        drag.snap = snap;
        if (snap) {
          drag.edit = { kind: "connect", call: drag.call, arg: drag.arg, target: snap.target, anchor: snap.anchor };
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
        break;
      }
      case "create": {
        drag.endSnap = isLineTool() ? findSnap(p) : undefined;
        drag.end = drag.endSnap ? pageToLocal(drag.frame, drag.transform, drag.endSnap.point) : snapPoint(drag.frame, drag.transform, p);
        break;
      }
    }
  }

  /** Snaps a move so the selection's first literal coordinate lands on the grid. */
  function snapDelta(dx: number, dy: number): Point {
    if (!editor.snap) return [dx, dy];
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
          editor.endDrag({ kind: "move", calls: editor.selected, dx: d.delta[0], dy: d.delta[1] });
        } else {
          editor.endDrag();
        }
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
      editor.focusInspector?.();
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
    let text: string;
    switch (editor.tool) {
      case "line":
      case "arrow":
        if (tiny) [x1, y1] = [x0 + 2, y0];
        text = `line(${pt(x0, y0)}, ${pt(x1, y1)}${editor.tool === "arrow" ? ', mark: (end: ">")' : ""})`;
        break;
      case "rect":
        if (tiny) [x1, y1] = [x0 + 2, y0 - 1];
        text = `rect(${pt(Math.min(x0, x1), Math.min(y0, y1))}, ${pt(Math.max(x0, x1), Math.max(y0, y1))})`;
        break;
      case "circle": {
        const r = tiny ? 0.5 : Math.hypot(x1 - x0, y1 - y0);
        text = `circle(${pt(x0, y0)}, radius: ${num(r)})`;
        break;
      }
      case "text":
        text = `content(${pt(x0, y0)}, [Text])`;
        break;
      default:
        return;
    }
    const steps: Parameters<Editor["chain"]>[0] = [{ kind: "insert", canvas: editor.activeCanvas ?? null, text }];
    for (const [arg, snap] of [[0, d.startSnap], [1, tiny ? undefined : d.endSnap]] as const) {
      if (snap) {
        steps.push(({ created, map }) => ({ kind: "connect", call: created[0], arg, target: map(snap.target), anchor: snap.anchor }));
      }
    }
    if (editor.chain(steps)) {
      editor.tool = "select";
      if (text.startsWith("content")) editor.focusInspector?.();
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
    drag?.kind === "handle" ? drag.snap : drag?.kind === "create" ? (drag.endSnap ?? drag.startSnap) : undefined,
  );

  // --- Grid ----------------------------------------------------------------

  /** The page-space area the grid covers: the page, or everything in view. */
  const gridArea = $derived.by(() => {
    if (!pageSize) return undefined;
    if (!editor.infinite) return { x0: 0, y0: 0, x1: pageSize.w, y1: pageSize.h };
    const [px, py] = editor.pan;
    return { x0: -px / editor.zoom, y0: -py / editor.zoom, x1: (width - px) / editor.zoom, y1: (height - py) / editor.zoom };
  });

  const grid = $derived.by(() => {
    if (!editor.showGrid || !gridArea || editor.activeCanvas === undefined) return undefined;
    const { x0, y0, x1, y1 } = gridArea;
    const frame = editor.frameFor(editor.activeCanvas);
    let step = editor.gridStep;
    while (step * frame.length * editor.zoom < 8) step *= 2;
    const [cx0, cy1] = editor.toCanvas(frame, [x0, y0]);
    const [cx1, cy0] = editor.toCanvas(frame, [x1, y1]);
    let d = "";
    for (let x = Math.ceil(cx0 / step) * step; x <= cx1; x += step) {
      const [px] = editor.toPage(frame, [x, 0]);
      d += `M${px},${y0}V${y1}`;
    }
    for (let y = Math.ceil(cy0 / step) * step; y <= cy1; y += step) {
      const [, py] = editor.toPage(frame, [0, y]);
      d += `M${x0},${py}H${x1}`;
    }
    return d;
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === " " && !(e.target instanceof HTMLInputElement) && !(e.target as HTMLElement).closest?.(".cm-editor")) {
      spaceHeld = true;
    }
  }
  function onkeyup(e: KeyboardEvent) {
    if (e.key === " ") spaceHeld = false;
  }

  const cursor = $derived(
    drag?.kind === "pan" || spaceHeld ? "grab" : editor.tool !== "select" ? "crosshair" : editor.hovered !== undefined ? "move" : "default",
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
  onpointerleave={() => (editor.hovered = undefined)}
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
      <g transform="translate({editor.pan[0]} {editor.pan[1]}) scale({editor.zoom})">
        {#if grid}<path class="grid" d={grid} />{/if}
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
            <path class="hovered" d={s.d} />
          {/if}
        {/each}

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

        {#each handles as h (h.arg)}
          {@const r = 4.5 / editor.zoom}
          {#if h.linked}
            <rect
              class="handle linked"
              data-handle="{h.call}:{h.arg}"
              x={h.point[0] - r}
              y={h.point[1] - r}
              width={2 * r}
              height={2 * r}
              transform="rotate(45 {h.point[0]} {h.point[1]})"
            />
          {:else}
            <circle class="handle" data-handle="{h.call}:{h.arg}" cx={h.point[0]} cy={h.point[1]} {r} />
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

        {#if activeSnap}
          <circle class="snap" cx={activeSnap.point[0]} cy={activeSnap.point[1]} r={6 / editor.zoom} />
        {/if}
      </g>
    </svg>
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
  .selected {
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
  .empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--muted);
  }
</style>
