<script lang="ts">
  // Properties of the selection, or the document outline when nothing is
  // selected. Every field commits on change (Enter / blur) as one edit.
  import { tick } from "svelte";
  import type { Editor } from "./editor.svelte";
  import { num } from "./format";
  import PropField from "./PropField.svelte";
  import { DEFAULTS, parseText } from "./props";
  import TextField from "./TextField.svelte";
  import { baseName, STATE_CALLS, type Arg, type Call } from "./scene";

  let { editor }: { editor: Editor } = $props();

  let primary: HTMLInputElement | HTMLTextAreaElement | undefined = $state();
  $effect(() => {
    editor.focusInspector = () => void tick().then(() => primary?.focus());
    return () => (editor.focusInspector = undefined);
  });

  const call = $derived(editor.selected.length === 1 ? editor.callById.get(editor.selected[0]) : undefined);

  const COMMON_KEYS = ["stroke", "fill", "mark", "radius", "padding", "frame", "anchor", "angle", "name"];

  function label(call: Call, arg: Arg, i: number): string {
    const positional = call.args.filter((a) => a.key === null);
    const index = positional.indexOf(arg);
    const base = baseName(call.callee);
    if (arg.value.type === "content" || parseText(arg.text)) return "Text";
    if (base === "content" || base === "circle" || base === "anchor") return index === (base === "anchor" ? 1 : 0) ? "Position" : `Argument ${i + 1}`;
    if (base === "rect") return index === 0 ? "Corner" : "Opposite corner";
    if (base === "line" || base === "bezier") return index === 0 ? "Start" : index === positional.length - 1 ? "End" : `Point ${index + 1}`;
    return `Point ${index + 1}`;
  }

  function summary(call: Call): string {
    if (call.name) return call.name;
    for (const a of call.args) {
      const text = a.key === null ? parseText(a.text) : null;
      if (text) return text.body;
    }
    return call.args[0]?.text ?? "";
  }

  const outline = $derived(
    editor.calls.map((c) => {
      let depth = 0;
      for (let p = c.parent; p !== null; p = editor.callById.get(p)?.parent ?? null) depth++;
      return { call: c, depth };
    }),
  );

  // --- Field commits -------------------------------------------------------

  function setCoord(call: Call, i: number, axis: "x" | "y", value: string) {
    const arg = call.args[i];
    const n = Number(value);
    if (arg.value.type !== "coord" || !Number.isFinite(n)) return;
    const x = axis === "x" ? n : arg.value.x;
    const y = axis === "y" ? n : arg.value.y;
    editor.edit({ kind: "set-coord", call: call.id, arg: i, x, y });
  }

  function setPoint(id: number, axis: "x" | "y", value: string) {
    const p = editor.pointById.get(id);
    const n = Number(value);
    if (!p || !Number.isFinite(n)) return;
    editor.edit({ kind: "set-point", point: id, x: axis === "x" ? n : p.x, y: axis === "y" ? n : p.y });
  }

  /** Replaces a use of a shared point with its current value, so it no longer follows. */
  function detach(call: Call, i: number) {
    const p = call.args[i].point === null ? undefined : editor.pointById.get(call.args[i].point!);
    if (p) editor.edit({ kind: "set-coord", call: call.id, arg: i, x: p.x, y: p.y });
  }

  function share(call: Call, i: number) {
    editor.edit({ kind: "extract-point", call: call.id, arg: i, name: null });
  }

  function pointName(id: number): string {
    const p = editor.pointById.get(id);
    return p ? (p.anchors[0] ?? p.path) : "";
  }

  function setArgText(call: Call, i: number, text: string) {
    if (text.trim() === "" || text === call.args[i].text) return;
    editor.edit({ kind: "set-arg-text", call: call.id, arg: i, text });
  }

  function setNamed(ids: number[], key: string, text: string | null) {
    if (text !== null && text.trim() === "") text = null;
    // Ids shift as earlier edits change the text; map each through the chain.
    editor.chain(ids.map((id) => ({ map }) => ({ kind: "set-named", call: map(id), key, text })));
  }

  function setName(call: Call, value: string) {
    const name = value.trim();
    if (name === (call.name ?? "")) return;
    setNamed([call.id], "name", name ? JSON.stringify(name) : null);
  }

  let newKey = $state("");
  let newValue = $state("");
  function addNamed(ids: number[]) {
    const key = newKey.trim();
    const value = newValue.trim() || DEFAULTS[key];
    if (!/^[a-zA-Z][\w-]*$/.test(key) || !value) return;
    setNamed(ids, key, value);
    newKey = "";
    newValue = "";
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !(e.target instanceof HTMLTextAreaElement && e.shiftKey)) {
      (e.target as HTMLElement).blur();
      e.preventDefault();
    }
    if (e.key === "Escape") (e.target as HTMLElement).blur();
  }
</script>

<div class="inspector">
  {#if call}
    {@const named = call.args.map((a, i) => [a, i] as const).filter(([a]) => a.key !== null && a.key !== "name")}
    <header>
      <span class="callee">{call.callee}</span>
      <input
        class="name"
        placeholder="name"
        value={call.name ?? ""}
        onchange={(e) => setName(call, e.currentTarget.value)}
        onkeydown={onKey}
        spellcheck="false"
      />
    </header>
    {#if call.in_loop}<p class="note">Inside a loop: edits apply to every iteration.</p>{/if}
    {#if call.conditional}<p class="note">Inside an <code>if</code>: only drawn when its condition holds.</p>{/if}

    <section>
      {#each call.args as arg, i (i)}
        {#if arg.key === null}
          <div class="row">
            <span class="label">{label(call, arg, i)}</span>
            {#if arg.point !== null && editor.pointById.get(arg.point)}
              {@const p = editor.pointById.get(arg.point)!}
              <div
                class="shared"
                role="group"
                onpointerenter={() => (editor.hoveredPoint = p.id)}
                onpointerleave={() => (editor.hoveredPoint = undefined)}
              >
                <div class="xy">
                  <label>x <input type="number" step={editor.gridStep} value={num(p.x)} onchange={(e) => setPoint(p.id, "x", e.currentTarget.value)} onkeydown={onKey} /></label>
                  <label>y <input type="number" step={editor.gridStep} value={num(p.y)} onchange={(e) => setPoint(p.id, "y", e.currentTarget.value)} onkeydown={onKey} /></label>
                </div>
                <div class="link">
                  <span class="chip" title="Shared point: edits move every shape that uses it">→ {pointName(p.id)}{#if p.path !== pointName(p.id)}<span class="path"> ({p.path})</span>{/if}</span>
                  <span class="users">{editor.pointUsers.get(p.id)?.length ?? 0} uses</span>
                  <button class="small" title="Use a copy of this point's position instead of the shared point" onclick={() => detach(call, i)}>Detach</button>
                </div>
              </div>
            {:else if arg.value.type === "coord"}
              <div class="xy with-action">
                <label>x <input type="number" step={editor.gridStep} value={num(arg.value.x)} onchange={(e) => setCoord(call, i, "x", e.currentTarget.value)} onkeydown={onKey} /></label>
                <label>y <input type="number" step={editor.gridStep} value={num(arg.value.y)} onchange={(e) => setCoord(call, i, "y", e.currentTarget.value)} onkeydown={onKey} /></label>
                <button class="small" title="Make this a shared point other shapes can use" onclick={() => share(call, i)}>Share</button>
              </div>
            {:else if parseText(arg.text)}
              <TextField bind:textarea={primary} text={arg.text} style={parseText(arg.text)!} commit={(t) => setArgText(call, i, t)} onkeydown={onKey} />
            {:else}
              <input class="code" value={arg.text} onchange={(e) => setArgText(call, i, e.currentTarget.value)} onkeydown={onKey} spellcheck="false" />
            {/if}
          </div>
        {/if}
      {/each}
    </section>

    <section>
      <h3>Properties</h3>
      {#each named as [arg] (arg.key)}
        {@const key = arg.key!}
        <div class="row">
          <span class="label">{key}</span>
          <div class="value">
            <PropField {key} text={arg.text} commit={(t) => setNamed([call.id], key, t)} onkeydown={onKey} />
            <button class="icon" title="Remove {key}" onclick={() => setNamed([call.id], key, null)}>×</button>
          </div>
        </div>
      {/each}
      <div class="row add">
        <input class="code key" placeholder="property" list="cetz-keys" bind:value={newKey} onkeydown={(e) => e.key === "Enter" && addNamed([call.id])} spellcheck="false" />
        <input class="code" placeholder="value" bind:value={newValue} onkeydown={(e) => e.key === "Enter" && addNamed([call.id])} spellcheck="false" />
        <button onclick={() => addNamed([call.id])}>Add</button>
      </div>
      {#if !STATE_CALLS.has(baseName(call.callee))}
        <div class="quick">
          {#if !call.args.some((a) => a.key === "fill")}
            <label>Fill <input type="color" value="#ffffff" onchange={(e) => setNamed([call.id], "fill", `rgb("${e.currentTarget.value}")`)} /></label>
          {/if}
          {#if !call.args.some((a) => a.key === "stroke")}
            <label>Stroke <input type="color" value="#000000" onchange={(e) => setNamed([call.id], "stroke", `rgb("${e.currentTarget.value}")`)} /></label>
          {/if}
        </div>
      {/if}
    </section>
  {:else if editor.selected.length > 1}
    {@const ids = editor.selected}
    <header><span class="callee">{ids.length} shapes</span></header>
    <section>
      <div class="quick">
        <label>Fill <input type="color" value="#ffffff" onchange={(e) => setNamed(ids, "fill", `rgb("${e.currentTarget.value}")`)} /></label>
        <label>Stroke <input type="color" value="#000000" onchange={(e) => setNamed(ids, "stroke", `rgb("${e.currentTarget.value}")`)} /></label>
      </div>
      <div class="row add">
        <input class="code key" placeholder="property" list="cetz-keys" bind:value={newKey} spellcheck="false" />
        <input class="code" placeholder="value" bind:value={newValue} onkeydown={(e) => e.key === "Enter" && addNamed(ids)} spellcheck="false" />
        <button onclick={() => addNamed(ids)}>Set</button>
      </div>
    </section>
  {:else}
    {#if editor.scene.points.length > 0}
      <header><span class="callee">Points</span></header>
      <ul class="points">
        {#each editor.scene.points as p (p.id)}
          <li
            class:hovered={editor.hoveredPoint === p.id}
            onpointerenter={() => (editor.hoveredPoint = p.id)}
            onpointerleave={() => (editor.hoveredPoint = undefined)}
          >
            <span class="pname" title={p.path}>{pointName(p.id)}</span>
            <label>x <input type="number" step={editor.gridStep} value={num(p.x)} onchange={(e) => setPoint(p.id, "x", e.currentTarget.value)} onkeydown={onKey} /></label>
            <label>y <input type="number" step={editor.gridStep} value={num(p.y)} onchange={(e) => setPoint(p.id, "y", e.currentTarget.value)} onkeydown={onKey} /></label>
            <span class="users">{editor.pointUsers.get(p.id)?.length ?? 0}</span>
          </li>
        {/each}
      </ul>
    {/if}
    <header><span class="callee">Shapes</span>{#if editor.scope !== undefined}<button class="link" onclick={() => (editor.scope = undefined)}>Exit group</button>{/if}</header>
    <ul class="outline">
      {#each outline as { call, depth } (call.id)}
        <li>
          <button
            class:hovered={editor.hovered === call.id}
            style:padding-left="{8 + depth * 14}px"
            onclick={() => {
              editor.scope = call.parent ?? undefined;
              editor.selection = [call.id];
            }}
            onpointerenter={() => (editor.hovered = call.id)}
            onpointerleave={() => (editor.hovered = undefined)}
          >
            <span class="callee small">{call.callee}</span>
            <span class="summary">{summary(call)}</span>
            {#if call.in_loop}<span class="badge">loop</span>{/if}
          </button>
        </li>
      {:else}
        <li class="empty">No CeTZ canvas in this file.</li>
      {/each}
    </ul>
  {/if}
  <datalist id="cetz-keys">
    {#each COMMON_KEYS as key}<option value={key}></option>{/each}
  </datalist>
</div>

<style>
  .inspector {
    padding: 10px 12px;
    font-size: 12.5px;
    overflow: auto;
    height: 100%;
    box-sizing: border-box;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .callee {
    font: 600 13px ui-monospace, "SF Mono", Menlo, monospace;
  }
  .callee.small {
    font-size: 12px;
    font-weight: 500;
  }
  .name {
    flex: 1;
  }
  h3 {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
    margin: 12px 0 6px;
  }
  .note {
    margin: 0 0 8px;
    color: var(--muted);
  }
  .row {
    display: grid;
    grid-template-columns: 92px minmax(0, 1fr);
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
  }
  .row.add {
    grid-template-columns: 92px minmax(0, 1fr) auto;
  }
  .label {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .xy {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }
  .xy label {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
  }
  .xy input {
    width: 100%;
    min-width: 0;
  }
  .xy.with-action {
    grid-template-columns: 1fr 1fr auto;
  }
  .shared {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .link {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .chip {
    color: var(--point);
    font: 600 11.5px ui-monospace, "SF Mono", Menlo, monospace;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip .path {
    color: var(--muted);
    font-weight: 400;
  }
  .users {
    color: var(--muted);
    font-size: 11px;
    white-space: nowrap;
  }
  button.small {
    padding: 1px 6px;
    font-size: 11.5px;
    margin-left: auto;
  }
  .points {
    list-style: none;
    margin: 0 0 12px;
    padding: 0;
  }
  .points li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 72px 72px 20px;
    gap: 6px;
    align-items: center;
    padding: 2px 4px;
    border-radius: 4px;
  }
  .points li.hovered {
    background: color-mix(in srgb, var(--point) 10%, transparent);
  }
  .points label {
    display: flex;
    align-items: center;
    gap: 3px;
    color: var(--muted);
  }
  .points input {
    width: 100%;
    min-width: 0;
  }
  .pname {
    font: 600 12px ui-monospace, "SF Mono", Menlo, monospace;
    color: var(--point);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .points .users {
    text-align: right;
  }
  .value {
    display: flex;
    gap: 4px;
    align-items: center;
    min-width: 0;
  }
  .inspector :global(input),
  .inspector :global(textarea) {
    font: inherit;
    color: inherit;
    background: var(--input-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 3px 6px;
    box-sizing: border-box;
  }
  .inspector :global(input:focus),
  .inspector :global(textarea:focus) {
    outline: 2px solid color-mix(in srgb, var(--accent) 40%, transparent);
    border-color: var(--accent);
  }
  .inspector :global(.code) {
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 12px;
  }
  .inspector :global(textarea) {
    width: 100%;
    resize: vertical;
  }
  input[type="color"] {
    width: 26px;
    height: 22px;
    padding: 1px;
    flex: none;
  }
  .quick {
    display: flex;
    gap: 12px;
    margin-top: 8px;
    color: var(--muted);
  }
  .quick label {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .inspector :global(button) {
    font: inherit;
    color: inherit;
    background: var(--button-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 3px 8px;
    cursor: pointer;
  }
  button.icon {
    padding: 0 6px;
    line-height: 20px;
  }
  button.link {
    border: none;
    background: none;
    color: var(--accent);
    padding: 0;
    margin-left: auto;
  }
  .outline {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .outline button {
    width: 100%;
    display: flex;
    gap: 8px;
    align-items: baseline;
    border: none;
    background: none;
    text-align: left;
    padding: 3px 8px;
    border-radius: 4px;
  }
  .outline button:hover,
  .outline button.hovered {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }
  .summary {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }
  .badge {
    margin-left: auto;
    font-size: 10px;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0 4px;
  }
  .empty {
    color: var(--muted);
    padding: 8px;
  }
</style>
