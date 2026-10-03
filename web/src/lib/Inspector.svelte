<script lang="ts">
  // Properties of the selection; the outline shows when nothing is
  // selected. Every field commits on change (Enter / blur) as one edit.
  import { tick } from "svelte";
  import type { Editor } from "./editor.svelte";
  import { num } from "./format";
  import PropRow from "./PropRow.svelte";
  import { applyChange, parseText, type Change } from "./props";
  import { optFor, optionsFor, wrappedOptions } from "./schema";
  import TextField from "./TextField.svelte";
  import { baseName, STATE_CALLS, type Arg, type Call } from "./scene";

  let { editor }: { editor: Editor } = $props();

  let primary: HTMLInputElement | HTMLTextAreaElement | undefined = $state();
  $effect(() => {
    editor.focusInspector = () => void tick().then(() => primary?.focus());
    return () => (editor.focusInspector = undefined);
  });

  // A rotated shape's scope shows the shape.
  const call = $derived(editor.selected.length === 1 ? (editor.rotatedShape(editor.selected[0]) ?? editor.callById.get(editor.selected[0])) : undefined);

  const ALIGN: { how: Parameters<Editor["alignSelection"]>[0]; title: string; icon: string }[] = [
    { how: "left", title: "Align left edges", icon: "M4 3v18M8 7h10v4H8zM8 14h6v4H8z" },
    { how: "center", title: "Align horizontal centres", icon: "M12 3v18M6 7h12v4H6zM9 14h6v4H9z" },
    { how: "right", title: "Align right edges", icon: "M20 3v18M6 7h10v4H6zM10 14h6v4h-6z" },
    { how: "top", title: "Align top edges", icon: "M3 4h18M7 8h4v10H7zM14 8h4v6h-4z" },
    { how: "middle", title: "Align vertical centres", icon: "M3 12h18M7 6h4v12H7zM14 9h4v6h-4z" },
    { how: "bottom", title: "Align bottom edges", icon: "M3 20h18M7 6h4v10H7zM14 10h4v6h-4z" },
    { how: "across", title: "Space evenly across (3 or more)", icon: "M3 4v16M21 4v16M9 8h6v8H9z" },
    { how: "down", title: "Space evenly down (3 or more)", icon: "M4 3h16M4 21h16M8 9h8v6H8z" },
  ];

  const COMMON_KEYS = ["stroke", "fill", "mark", "radius", "padding", "frame", "anchor", "angle", "name"];

  function label(call: Call, arg: Arg, i: number): string {
    const positional = call.args.filter((a) => a.key === null);
    const index = positional.indexOf(arg);
    const base = baseName(call.callee);
    if (arg.value.type === "content" || parseText(arg.text)) return "Text";
    if (base === "content" || base === "circle" || base === "anchor") return index === (base === "anchor" ? 1 : 0) ? "Position" : `Argument ${i + 1}`;
    if (base === "rect") return index === 0 ? "Corner" : "Opposite corner";
    if (base === "line" || base === "bezier" || base === "catmull" || base === "hobby") return index === 0 ? "Start" : index === positional.length - 1 ? "End" : `Point ${index + 1}`;
    if (base === "brace" || base === "flat-brace") return index === 0 ? "Start" : "End";
    if (base === "polygon" || base === "n-star") return index === 0 ? "Center" : "Sides";
    if (base === "arc") return "Position";
    if (base === "angle" || base === "right-angle") return ["Origin", "Side a", "Side b"][index] ?? `Point ${index + 1}`;
    return `Point ${index + 1}`;
  }

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

  /** Sets a named argument on each call, applying `change` to that call's own value. */
  function setNamed(ids: number[], key: string, change: Change) {
    const edits = ids.flatMap((id) => {
      const call = editor.callById.get(id);
      if (!call) return [];
      const old = valueOf(call, key);
      let text = applyChange(change, old);
      if (text !== null && text.trim() === "") text = null;
      return (text ?? "") === old ? [] : [{ id, text }];
    });
    // Ids shift as earlier edits change the text; map each through the chain.
    if (edits.length > 0) editor.chain(edits.map(({ id, text }) => ({ map }) => ({ kind: "set-named", call: map(id), key, text })));
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
    const value = newValue.trim();
    if (!/^[a-zA-Z][\w-]*$/.test(key) || !value) return;
    setNamed(ids, key, value);
    newKey = "";
    newValue = "";
  }

  /** The options this call takes, and any other named arguments it has. */
  function options(call: Call) {
    const base = baseName(call.callee);
    const fn = editor.scene.variables.find((v) => v.kind === "function" && v.name === call.callee);
    const groups = (fn && wrappedOptions(editor.index.slice(fn.value_range.start, fn.value_range.end))) ?? optionsFor(base, STATE_CALLS.has(base));
    const known = new Set(groups.flatMap((g) => g.opts.map((o) => o.key)));
    const other = call.args.filter((a) => a.key !== null && a.key !== "name" && !known.has(a.key)).map((a) => optFor(a.key!));
    return { groups, other };
  }

  /** The options every one of these calls takes, and named arguments they all have. */
  function sharedOptions(calls: Call[]) {
    const each = calls.map(options);
    const takes = each.map((o) => new Set([...o.groups.flatMap((g) => g.opts), ...o.other].map((opt) => opt.key)));
    const shared = (key: string) => takes.every((keys) => keys.has(key));
    const groups = (each[0]?.groups ?? []).map((g) => ({ ...g, opts: g.opts.filter((o) => shared(o.key)) })).filter((g) => g.opts.length > 0);
    const other = (each[0]?.other ?? []).filter((o) => shared(o.key));
    return { groups, other };
  }

  function valueOf(call: Call, key: string): string {
    return call.args.find((a) => a.key === key)?.text ?? "";
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
    {@const opts = options(call)}
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
      {#if editor.isGroup(call.id)}<button title="Put its shapes back in place of the group (⇧⌘G)" onclick={() => editor.ungroupSelection()}>Ungroup</button>{/if}
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

    {#each opts.groups as group (group.title)}
      <section>
        <h3>{group.title}</h3>
        {#each group.opts as opt (opt.key)}
          <PropRow {opt} texts={[valueOf(call, opt.key)]} commit={(c) => setNamed([call.id], opt.key, c)} onkeydown={onKey} />
        {/each}
      </section>
    {/each}
    <section>
      {#if opts.other.length > 0 || opts.groups.length === 0}<h3>Other</h3>{/if}
      {#each opts.other as opt (opt.key)}
        <PropRow {opt} texts={[valueOf(call, opt.key)]} commit={(c) => setNamed([call.id], opt.key, c)} onkeydown={onKey} />
      {/each}
      <div class="row add">
        <input class="code key" placeholder="property" list="cetz-keys" bind:value={newKey} onkeydown={(e) => e.key === "Enter" && addNamed([call.id])} spellcheck="false" />
        <input class="code" placeholder="value" bind:value={newValue} onkeydown={(e) => e.key === "Enter" && addNamed([call.id])} spellcheck="false" />
        <button onclick={() => addNamed([call.id])}>Add</button>
      </div>
    </section>
  {:else if editor.selected.length > 1}
    {@const ids = editor.selected}
    <header>
      <span class="callee">{ids.length} shapes</span>
      <span class="spacer"></span>
      {#if ids.some((id) => editor.isGroup(id))}<button title="Put each group's shapes back in its place (⇧⌘G)" onclick={() => editor.ungroupSelection()}>Ungroup</button>{/if}
      <button title="Wrap these in a group (⌘G)" onclick={() => editor.groupSelection()}>Group</button>
    </header>
    <section class="align">
      {#each ALIGN as a (a.how)}
        <button
          class="icon"
          title={a.title}
          disabled={(a.how === "across" || a.how === "down") && ids.length < 3}
          onclick={() => editor.alignSelection(a.how)}><svg viewBox="0 0 24 24"><path d={a.icon} /></svg></button
        >
      {/each}
    </section>
    <!-- Rotated shapes' scopes edit as their shapes. -->
    {@const shapes = ids.map((id) => editor.rotatedShape(id)?.id ?? id)}
    {@const calls = shapes.map((id) => editor.callById.get(id)).filter((c) => c !== undefined)}
    {@const opts = sharedOptions(calls)}
    {#if opts.groups.length === 0 && opts.other.length === 0}<p class="note">These shapes have no options in common.</p>{/if}
    {#each opts.groups as group (group.title)}
      <section>
        <h3>{group.title}</h3>
        {#each group.opts as opt (opt.key)}
          <PropRow {opt} texts={calls.map((c) => valueOf(c, opt.key))} commit={(c) => setNamed(shapes, opt.key, c)} onkeydown={onKey} />
        {/each}
      </section>
    {/each}
    <section>
      {#if opts.other.length > 0}<h3>Other</h3>{/if}
      {#each opts.other as opt (opt.key)}
        <PropRow {opt} texts={calls.map((c) => valueOf(c, opt.key))} commit={(c) => setNamed(shapes, opt.key, c)} onkeydown={onKey} />
      {/each}
      <div class="row add">
        <input class="code key" placeholder="property" list="cetz-keys" bind:value={newKey} spellcheck="false" />
        <input class="code" placeholder="value" bind:value={newValue} onkeydown={(e) => e.key === "Enter" && addNamed(shapes)} spellcheck="false" />
        <button onclick={() => addNamed(shapes)}>Set</button>
      </div>
    </section>
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
  .name,
  .spacer {
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
  /* Text fields are tall; keep their label at the top. */
  .row:has(:global(.text)) {
    align-items: start;
  }
  .row:has(:global(.text)) .label {
    padding-top: 4px;
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
  .align {
    display: flex;
    gap: 3px;
  }
  .align button {
    flex: 1;
    display: grid;
    place-items: center;
    padding: 3px 0;
  }
  .align button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .align svg {
    width: 18px;
    height: 18px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
  }
  button.small {
    padding: 1px 6px;
    font-size: 11.5px;
    margin-left: auto;
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
  .inspector :global(button) {
    font: inherit;
    color: inherit;
    background: var(--button-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 3px 8px;
    cursor: pointer;
  }
</style>
