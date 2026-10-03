<script lang="ts">
  // The document outline, shown when nothing is selected: variables (the
  // definitions, editable when they're literal points) and shapes (the draw
  // calls; loops expand into the repetitions CeTZ drew, read-only).
  import type { Editor } from "./editor.svelte";
  import { num } from "./format";
  import { isVec, type Probe, type Vec3 } from "./probe";
  import { baseName, type Call, type Point, type Variable } from "./scene";

  let { editor }: { editor: Editor } = $props();

  /** Variables folded closed, and loops unfolded, by the user. */
  let folded = $state(new Set<string>());
  let unfolded = $state(new Set<number>());

  function toggle<T>(set: Set<T>, key: T): Set<T> {
    const next = new Set(set);
    if (!next.delete(key)) next.add(key);
    return next;
  }

  // --- Variables -----------------------------------------------------------

  function pointsOf(v: Variable): Point[] {
    return editor.scene.points.filter((p) => p.path === v.name || p.path.startsWith(`${v.name}.`) || p.path.startsWith(`${v.name}[`));
  }

  /** `pts.A` → `A`, `ps[0]` → `[0]`. */
  function entryName(v: Variable, p: Point): string {
    const rest = p.path.slice(v.name.length);
    return rest.startsWith(".") ? rest.slice(1) : rest || v.name;
  }

  function setPoint(id: number, axis: "x" | "y", value: string) {
    const p = editor.pointById.get(id);
    const n = Number(value);
    if (!p || !Number.isFinite(n)) return;
    editor.edit({ kind: "set-point", point: id, x: axis === "x" ? n : p.x, y: axis === "y" ? n : p.y });
  }

  function jumpTo(v: Variable) {
    editor.openCode?.();
    editor.code?.select(v.range);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" || e.key === "Escape") (e.target as HTMLElement).blur();
  }

  // --- Shapes --------------------------------------------------------------

  const rows = $derived(
    editor.calls.map((call) => {
      let depth = 0;
      for (let p = call.parent; p !== null; p = editor.callById.get(p)?.parent ?? null) depth++;
      return { call, depth };
    }),
  );

  /** Points a call defines itself, e.g. `anchor("C", (5, 5))`. */
  function definedBy(call: Call): Point[] {
    return editor.scene.points.filter((p) => p.range.start >= call.range.start && p.range.end <= call.range.end && !isVariablePoint(p));
  }

  function isVariablePoint(p: Point): boolean {
    return editor.scene.variables.some((v) => p.path === v.name || p.path.startsWith(`${v.name}.`) || p.path.startsWith(`${v.name}[`));
  }

  function summary(call: Call): string {
    if (call.name) return call.name;
    const content = call.args.find((a) => a.value.type === "content");
    if (content?.value.type === "content") return editor.index.slice(content.value.inner.start, content.value.inner.end);
    return call.args.map((a) => a.text).slice(0, 4).join(" ");
  }

  function loopLabel(call: Call): string {
    const loop = editor.scene.loops.find((l) => l.id === call.loop_id);
    if (!loop) return "loop";
    const iterable = loop.iterable.length > 24 ? `${loop.iterable.slice(0, 24)}…` : loop.iterable;
    return loop.pattern ? `for ${loop.pattern} in ${iterable}` : `while ${iterable}`;
  }

  /** Page positions of every shared point, for naming loop repetitions. */
  const pointPlaces = $derived(
    editor.scene.points.flatMap((p) => {
      const placed = editor.placePoint(p.id);
      return placed ? [{ name: p.anchors[0] ?? p.path, page: placed.page }] : [];
    }),
  );

  function nameAt(probe: Probe, v: Vec3): string | undefined {
    const [x, y] = editor.toPage({ origin: probe.origin, length: probe.length }, v);
    return pointPlaces.find((p) => Math.hypot(p.page[0] - x, p.page[1] - y) < 0.5)?.name;
  }

  /** A readable label for one repetition: its anchor name, or the points it joins. */
  function instanceLabel(probe: Probe, index: number): string {
    if (probe.drawables.length === 0 && probe.name) {
      const point = editor.scene.points.find((p) => p.anchors.includes(probe.name!));
      return point && point.path !== probe.name ? `${probe.name} → ${point.path}` : probe.name;
    }
    const path = probe.drawables.find((d) => d.type === "path");
    if (path?.type === "path" && path.segments.length > 0) {
      const [origin, closed, segments] = path.segments[0];
      const vertices = [origin, ...segments.map((s) => s[s.length - 1] as Vec3)];
      const names = vertices.map((v) => nameAt(probe, v));
      if (names.every((n) => n !== undefined)) {
        const unique = closed && names.length > 1 && names[0] === names[names.length - 1] ? names.slice(0, -1) : names;
        return unique.join(closed ? "–" : " → ");
      }
    }
    const content = probe.drawables.find((d) => d.type === "content");
    if (content?.type === "content" && isVec(content.pos)) return `at (${num(content.pos[0])}, ${num(content.pos[1])})`;
    return `#${index + 1}`;
  }

  // --- Icons ---------------------------------------------------------------

  type Kind = "line" | "arrow" | "rect" | "circle" | "polygon" | "curve" | "text" | "anchor" | "group" | "style" | "transform" | "other";

  const ICONS: Record<Kind, string> = {
    line: "M5 19L19 5",
    arrow: "M5 19L19 5M11 5h8v8",
    rect: "M4 6h16v12H4z",
    circle: "M12 4a8 8 0 1 0 0.01 0z",
    polygon: "M12 4l8 6-3 9H7l-3-9z",
    curve: "M4 18C8 4 16 20 20 6",
    text: "M5 7V5h14v2M12 5v14M9 19h6",
    anchor: "M12 3v18M3 12h18M12 9a3 3 0 1 0 0.01 0z",
    group: "M8 4H5v16h3M16 4h3v16h-3",
    style: "M4 20l4-1 10-10-3-3L5 16zM13 7l3 3",
    transform: "M19 12a7 7 0 1 1-2-4.9M19 4v4h-4",
    other: "M8 12h.01M12 12h.01M16 12h.01",
  };

  const VARIABLE_ICONS: Record<Variable["kind"], string> = {
    point: "M12 9a3 3 0 1 0 0.01 0z",
    points: "M9 4C6 4 7 9 4 12c3 3 2 8 5 8M15 4c3 0 2 5 5 8-3 3-2 8-5 8",
    function: "M15 4c-3 0-4 2-4 5v8c0 2-1 3-3 3M7 11h8",
    value: "M6 9h12M6 15h12",
  };

  const BY_NAME: Record<string, Kind> = {
    line: "line",
    rect: "rect",
    grid: "rect",
    circle: "circle",
    "circle-through": "circle",
    ellipse: "circle",
    arc: "curve",
    "arc-through": "curve",
    bezier: "curve",
    "bezier-through": "curve",
    catmull: "curve",
    hobby: "curve",
    "merge-path": "curve",
    polygon: "polygon",
    "n-star": "polygon",
    content: "text",
    text: "text",
    anchor: "anchor",
    "copy-anchors": "anchor",
    group: "group",
    scope: "group",
    "on-layer": "group",
    floating: "group",
    hide: "group",
    "set-style": "style",
    "set-ctx": "style",
    translate: "transform",
    rotate: "transform",
    scale: "transform",
    "set-origin": "transform",
    "set-transform": "transform",
    "set-viewport": "transform",
  };

  /** What a call draws: by its name for CeTZ's own functions, else from what the probe saw. */
  function kindOf(call: Call): Kind {
    const known = BY_NAME[baseName(call.callee)];
    if (known === "line") return call.args.some((a) => a.key === "mark") ? "arrow" : "line";
    if (known) return known;
    if (editor.calls.some((c) => c.parent === call.id)) return "group";
    const probe = editor.probesById.get(call.id)?.[0];
    if (!probe || probe.drawables.length === 0) return "other";
    const paths = probe.drawables.filter((d) => d.type === "path");
    if (paths.length === 0) return "text";
    const first = paths[0];
    if (first.type !== "path" || first.segments.length === 0) return "other";
    const [, closed, segments] = first.segments[0];
    if (segments.some((s) => s[0] === "c")) return "curve";
    if (closed) return "polygon";
    return call.args.some((a) => a.key === "mark") || paths.length > 1 ? "arrow" : "line";
  }

  // Hovers from here get the canvas's stronger (orange) highlight.
  function hoverShape(id: number) {
    editor.hoverSource = "panel";
    editor.hovered = id;
  }

  function hoverPoint(id: number) {
    editor.hoverSource = "panel";
    editor.hoveredPoint = id;
  }

  function select(call: Call) {
    editor.scope = call.parent ?? undefined;
    editor.selection = [call.id];
  }
</script>

<div class="outline">
  {#if editor.scene.variables.length > 0}
    <h3>Variables</h3>
    <ul>
      {#each editor.scene.variables as v (v.range.start)}
        {@const points = pointsOf(v)}
        {#if v.kind === "point" && points.length === 1}
          {@const p = points[0]}
          <li
            class="row point"
            class:hovered={editor.hoveredPoint === p.id}
            onpointerenter={() => hoverPoint(p.id)}
            onpointerleave={() => (editor.hoveredPoint = undefined)}
          >
            <span class="name point-name"><span class="icon var-icon point-icon"><svg viewBox="0 0 24 24"><path d={VARIABLE_ICONS.point} /></svg></span>{v.name}</span>
            <label>x <input type="number" step={editor.gridStep} value={num(p.x)} onchange={(e) => setPoint(p.id, "x", e.currentTarget.value)} onkeydown={onKey} /></label>
            <label>y <input type="number" step={editor.gridStep} value={num(p.y)} onchange={(e) => setPoint(p.id, "y", e.currentTarget.value)} onkeydown={onKey} /></label>
            <span class="uses" title="Shapes using it">{editor.pointUsers.get(p.id)?.length ?? 0}</span>
          </li>
        {:else if v.kind === "points"}
          {@const open = !folded.has(v.name)}
          <li>
            <button class="row group" onclick={() => (folded = toggle(folded, v.name))} aria-expanded={open}>
              <span class="chevron" class:open>›</span>
              <span class="icon var-icon point-icon"><svg viewBox="0 0 24 24"><path d={VARIABLE_ICONS.points} /></svg></span>
              <span class="name">{v.name}</span>
              <span class="meta">{points.length} points</span>
            </button>
            {#if open}
              <ul>
                {#each points as p (p.id)}
                  <li
                    class="row point child"
                    class:hovered={editor.hoveredPoint === p.id}
                    onpointerenter={() => hoverPoint(p.id)}
                    onpointerleave={() => (editor.hoveredPoint = undefined)}
                  >
                    <span class="name point-name" title={p.anchors.length ? `anchor ${p.anchors.join(", ")}` : p.path}>{entryName(v, p)}</span>
                    <label>x <input type="number" step={editor.gridStep} value={num(p.x)} onchange={(e) => setPoint(p.id, "x", e.currentTarget.value)} onkeydown={onKey} /></label>
                    <label>y <input type="number" step={editor.gridStep} value={num(p.y)} onchange={(e) => setPoint(p.id, "y", e.currentTarget.value)} onkeydown={onKey} /></label>
                    <span class="uses" title="Shapes using it">{editor.pointUsers.get(p.id)?.length ?? 0}</span>
                  </li>
                {/each}
              </ul>
            {/if}
          </li>
        {:else}
          <li>
            <button class="row readonly" onclick={() => jumpTo(v)} title="Show in code">
              <span class="chevron-space"></span>
              <span class="icon var-icon"><svg viewBox="0 0 24 24"><path d={VARIABLE_ICONS[v.kind]} /></svg></span>
              <span class="name">{v.name}</span>
              <span class="kind">{v.kind === "function" ? "function" : ""}</span>
              <span class="meta code">{v.summary}</span>
            </button>
          </li>
        {/if}
      {/each}
    </ul>
  {/if}

  <h3>
    Shapes
    {#if editor.scope !== undefined}<button class="link" onclick={() => (editor.scope = undefined)}>Exit group</button>{/if}
  </h3>
  <ul>
    {#each rows as { call, depth } (call.id)}
      {@const instances = editor.probesById.get(call.id) ?? []}
      {@const looped = call.in_loop}
      {@const kind = kindOf(call)}
      {@const defined = definedBy(call)}
      {@const open = unfolded.has(call.id)}
      <li>
        <div class="row shape" class:hovered={editor.hovered === call.id} style:padding-left="{4 + depth * 14}px">
          {#if looped || defined.length > 0}
            <button class="chevron-button" onclick={() => (unfolded = toggle(unfolded, call.id))} aria-expanded={open} aria-label="Expand">
              <span class="chevron" class:open>›</span>
            </button>
          {:else}
            <span class="chevron-space"></span>
          {/if}
          <button
            class="shape-button"
            onclick={() => select(call)}
            onpointerenter={() => hoverShape(call.id)}
            onpointerleave={() => (editor.hovered = undefined)}
          >
            <span class="icon" title={kind}>
              <svg viewBox="0 0 24 24"><path d={ICONS[kind]} /></svg>
              {#if looped}
                <span class="flow" title="Drawn by a loop">↻</span>
              {:else if call.conditional}
                <span class="flow" title="Only drawn when its condition holds">⑂</span>
              {/if}
            </span>
            <span class="name">{call.callee}</span>
            {#if looped}<span class="pill" title="Shapes this call drew">×{instances.length}</span>{/if}
            <span class="meta">{looped ? loopLabel(call) : summary(call)}</span>
          </button>
        </div>
        {#if open}
          <ul class="children" style:--indent="{depth * 14}px">
            {#each defined as p (p.id)}
              <li
                class="row point child"
                class:hovered={editor.hoveredPoint === p.id}
                style:padding-left="{22 + depth * 14}px"
                onpointerenter={() => hoverPoint(p.id)}
                onpointerleave={() => (editor.hoveredPoint = undefined)}
              >
                <span class="name point-name">{p.anchors[0] ?? p.path}</span>
                <label>x <input type="number" step={editor.gridStep} value={num(p.x)} onchange={(e) => setPoint(p.id, "x", e.currentTarget.value)} onkeydown={onKey} /></label>
                <label>y <input type="number" step={editor.gridStep} value={num(p.y)} onchange={(e) => setPoint(p.id, "y", e.currentTarget.value)} onkeydown={onKey} /></label>
                <span class="uses">{editor.pointUsers.get(p.id)?.length ?? 0}</span>
              </li>
            {/each}
            {#if looped}
              {#each instances as probe, index (index)}
                {@const focused = editor.focusedInstance?.call === call.id && editor.focusedInstance.index === index}
                <li>
                  <button
                    class="row instance"
                    class:focused
                    class:hovered={editor.hoveredInstance?.call === call.id && editor.hoveredInstance.index === index}
                    style:padding-left="{30 + depth * 14}px"
                    title="Drawn by the loop; edit its points or the call to change it"
                    onclick={() => (editor.focusedInstance = focused ? undefined : { call: call.id, index })}
                    onpointerenter={() => (editor.hoveredInstance = { call: call.id, index })}
                    onpointerleave={() => (editor.hoveredInstance = undefined)}
                  >
                    <span class="meta code">{instanceLabel(probe, index)}</span>
                  </button>
                </li>
              {/each}
            {/if}
          </ul>
        {/if}
      </li>
    {:else}
      <li class="empty">No CeTZ canvas in this file.</li>
    {/each}
  </ul>
</div>

<style>
  .outline {
    padding: 8px 10px;
    font-size: 12.5px;
    overflow: auto;
    height: 100%;
    box-sizing: border-box;
  }
  h3 {
    display: flex;
    align-items: center;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
    margin: 6px 4px 4px;
  }
  ul {
    list-style: none;
    margin: 0 0 8px;
    padding: 0;
  }
  ul ul {
    margin: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    min-height: 24px;
    padding: 1px 4px;
    border-radius: 4px;
    box-sizing: border-box;
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: none;
    text-align: left;
    cursor: pointer;
    padding: 0;
  }
  button.row,
  .shape-button {
    min-width: 0;
  }
  button.row:hover,
  .row.hovered,
  .row.shape:hover {
    background: color-mix(in srgb, var(--accent) 9%, transparent);
  }
  .row.point.hovered {
    background: color-mix(in srgb, var(--point) 10%, transparent);
  }
  .row.point {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 70px 70px 18px;
  }
  .row.child {
    padding-left: 22px;
  }
  .shape-button {
    flex: 1;
    display: flex;
    gap: 8px;
    align-items: baseline;
    padding: 2px 0;
  }
  .name {
    font: 500 12px ui-monospace, "SF Mono", Menlo, monospace;
    white-space: nowrap;
  }
  .point-name {
    color: var(--point);
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .kind {
    color: var(--muted);
    font-size: 11px;
  }
  .meta {
    color: var(--muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .meta.code {
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 11.5px;
  }
  .row.readonly .meta {
    margin-left: auto;
  }
  .uses {
    color: var(--muted);
    font-size: 11px;
    text-align: right;
  }
  .chevron {
    display: inline-block;
    width: 10px;
    color: var(--muted);
    transition: transform 0.1s;
  }
  .chevron.open {
    transform: rotate(90deg);
  }
  .chevron-button {
    width: 14px;
    flex: none;
  }
  .chevron-space {
    width: 14px;
    flex: none;
  }
  /* What each row draws, with loop and condition markers as badges. */
  .icon {
    position: relative;
    flex: none;
    width: 16px;
    height: 16px;
    align-self: center;
    color: var(--muted);
  }
  .icon svg {
    display: block;
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .row.shape:hover .icon,
  .row.shape.hovered .icon {
    color: var(--text);
  }
  .var-icon {
    margin-right: 6px;
  }
  .point-name .var-icon {
    display: inline-block;
    vertical-align: -3px;
  }
  .point-icon {
    color: var(--point);
  }
  .point-icon svg {
    stroke-width: 2.4;
  }
  .flow {
    position: absolute;
    right: -5px;
    bottom: -4px;
    display: grid;
    place-items: center;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 1px solid var(--snap);
    background: var(--panel);
    color: var(--snap);
    font-size: 8px;
    font-weight: 700;
    line-height: 1;
  }
  .pill {
    flex: none;
    padding: 0 5px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--snap) 14%, transparent);
    color: var(--snap);
    font: 600 10.5px ui-monospace, "SF Mono", Menlo, monospace;
    line-height: 16px;
  }

  /* Repetitions: derived, read-only, hung off a dotted guide line. */
  .children {
    position: relative;
  }
  .children::before {
    content: "";
    position: absolute;
    top: 0;
    bottom: 6px;
    left: calc(var(--indent) + 17px);
    border-left: 1px dotted var(--muted);
    opacity: 0.6;
  }
  .row.instance {
    position: relative;
  }
  .row.instance::before {
    content: "";
    position: absolute;
    left: calc(var(--indent) + 14px);
    width: 6px;
    height: 6px;
    border-radius: 50%;
    border: 1px solid var(--muted);
    background: var(--panel);
  }
  .row.instance .meta {
    color: var(--muted);
  }
  .row.instance.hovered,
  .row.instance.focused {
    background: color-mix(in srgb, var(--snap) 12%, transparent);
  }
  .row.instance.hovered .meta,
  .row.instance.focused .meta {
    color: var(--snap);
  }
  .row.instance.hovered::before,
  .row.instance.focused::before {
    border-color: var(--snap);
    background: var(--snap);
  }
  label {
    display: flex;
    align-items: center;
    gap: 3px;
    color: var(--muted);
  }
  input {
    width: 100%;
    min-width: 0;
    font: inherit;
    color: inherit;
    background: var(--input-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 2px 4px;
    box-sizing: border-box;
  }
  .link {
    color: var(--accent);
    margin-left: auto;
    text-transform: none;
    letter-spacing: 0;
    font-size: 12px;
  }
  .empty {
    color: var(--muted);
    padding: 8px;
  }
</style>
