<script lang="ts">
  // The document outline, shown when nothing is selected: variables (the
  // definitions, editable when they're literal points) and shapes (the draw
  // calls; loops expand into the repetitions CeTZ drew, read-only).
  import type { Editor } from "./editor.svelte";
  import { num } from "./format";
  import { isVec, type Probe, type Vec3 } from "./probe";
  import type { Call, Point, Variable } from "./scene";

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
            onpointerenter={() => (editor.hoveredPoint = p.id)}
            onpointerleave={() => (editor.hoveredPoint = undefined)}
          >
            <span class="name point-name">{v.name}</span>
            <label>x <input type="number" step={editor.gridStep} value={num(p.x)} onchange={(e) => setPoint(p.id, "x", e.currentTarget.value)} onkeydown={onKey} /></label>
            <label>y <input type="number" step={editor.gridStep} value={num(p.y)} onchange={(e) => setPoint(p.id, "y", e.currentTarget.value)} onkeydown={onKey} /></label>
            <span class="uses" title="Shapes using it">{editor.pointUsers.get(p.id)?.length ?? 0}</span>
          </li>
        {:else if v.kind === "points"}
          {@const open = !folded.has(v.name)}
          <li>
            <button class="row group" onclick={() => (folded = toggle(folded, v.name))} aria-expanded={open}>
              <span class="chevron" class:open>›</span>
              <span class="name">{v.name}</span>
              <span class="meta">{points.length} points</span>
            </button>
            {#if open}
              <ul>
                {#each points as p (p.id)}
                  <li
                    class="row point child"
                    class:hovered={editor.hoveredPoint === p.id}
                    onpointerenter={() => (editor.hoveredPoint = p.id)}
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
            onpointerenter={() => (editor.hovered = call.id)}
            onpointerleave={() => (editor.hovered = undefined)}
          >
            <span class="name">{call.callee}{#if looped}<span class="count"> ×{instances.length}</span>{/if}</span>
            <span class="meta">{looped ? loopLabel(call) : summary(call)}</span>
          </button>
        </div>
        {#if open}
          <ul>
            {#each defined as p (p.id)}
              <li
                class="row point child"
                class:hovered={editor.hoveredPoint === p.id}
                style:padding-left="{22 + depth * 14}px"
                onpointerenter={() => (editor.hoveredPoint = p.id)}
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
                    style:padding-left="{26 + depth * 14}px"
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
  .count {
    color: var(--muted);
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
  .row.instance.focused {
    background: color-mix(in srgb, var(--snap) 14%, transparent);
  }
  .row.instance .meta {
    color: var(--text);
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
