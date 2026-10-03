<script lang="ts">
  // The value of one named argument. Known keys whose value we can read get
  // fields (stroke: color/thickness/dash, mark: start/end, frame, ...); the
  // </> button, or a value we can't read, shows the plain code instead.
  import ColorInput from "./ColorInput.svelte";
  import { num } from "./format";
  import {
    ANCHORS,
    DASHES,
    FRAMES,
    MARKS,
    lengthText,
    markEnd,
    markSet,
    parseExpr,
    parseMark,
    parseStroke,
    strokeGet,
    strokeSet,
  } from "./props";

  let {
    key,
    text,
    commit,
    onkeydown,
  }: {
    key: string;
    text: string;
    commit: (text: string | null) => void;
    onkeydown: (e: KeyboardEvent) => void;
  } = $props();

  /** Keys whose value is a color. */
  const COLOR_KEYS = new Set(["fill", "paint", "color"]);
  /** Keys whose value is a plain number, and the step for each. */
  const NUMBER_KEYS: Record<string, number> = { padding: 0.1, radius: 0.1, angle: 15, scale: 0.1, "line-spacing": 0.1 };

  let raw = $state(false);
  const node = $derived(parseExpr(text));
  const stroke = $derived(key === "stroke" ? parseStroke(text) : null);
  const mark = $derived(key === "mark" ? parseMark(text) : null);

  const view = $derived.by(() => {
    if (COLOR_KEYS.has(key)) return "color";
    if (raw || !node) return "code";
    if (stroke) return "stroke";
    if (mark) return "mark";
    if (key === "frame" && ((node.kind === "str" && FRAMES.includes(node.value)) || node.kind === "none")) return "frame";
    if (key === "anchor" && node.kind === "str") return "anchor";
    if (key in NUMBER_KEYS && (node.kind === "number" || node.kind === "numeric")) return "number";
    if (node.kind === "bool") return "bool";
    return "code";
  });

  const str = (s: string) => JSON.stringify(s);

  function setNumber(value: string) {
    const n = Number(value);
    if (!Number.isFinite(n) || value.trim() === "") return;
    commit(node?.kind === "numeric" ? `${num(n)}${node.unit}` : num(n));
  }
</script>

<div class="field">
  {#if view === "color"}
    <ColorInput {text} {commit} {onkeydown} title={key} />
  {:else if view === "stroke" && stroke}
    {@const paint = strokeGet(stroke, "paint")}
    {@const dash = strokeGet(stroke, "dash")}
    {@const current = dash?.node.kind === "str" ? dash.node.value : dash ? dash.text : ""}
    <ColorInput text={stroke.none ? "none" : (paint?.text ?? "")} code={false} title="Stroke color" commit={(t) => commit(strokeSet(stroke, "paint", t))} />
    <input
      class="code length"
      title="Thickness"
      placeholder={stroke.none ? "none" : "auto"}
      value={strokeGet(stroke, "thickness")?.text ?? ""}
      onchange={(e) => commit(strokeSet(stroke, "thickness", lengthText(e.currentTarget.value)))}
      {onkeydown}
      spellcheck="false"
    />
    <select title="Dash pattern" onchange={(e) => commit(strokeSet(stroke, "dash", e.currentTarget.value ? str(e.currentTarget.value) : null))}>
      <option value="" selected={current === ""}>solid</option>
      {#each DASHES.slice(1) as d}<option value={d} selected={current === d}>{d}</option>{/each}
      {#if current && !DASHES.includes(current)}<option value={current} selected>{current}</option>{/if}
    </select>
  {:else if view === "mark" && mark}
    {@const fill = mark.find((e) => e.key === "fill")}
    {#each ["start", "end"] as const as end}
      {@const current = markEnd(mark, end)}
      <select title="{end === 'start' ? 'Start' : 'End'} mark" onchange={(e) => commit(markSet(mark, end, e.currentTarget.value ? str(e.currentTarget.value) : null))}>
        <option value="" selected={current === ""}>none</option>
        {#each MARKS as [sym, name]}<option value={sym} selected={current === sym}>{name}</option>{/each}
        {#if current && !MARKS.some(([s]) => s === current)}<option value={current} selected>{current}</option>{/if}
      </select>
    {/each}
    <ColorInput text={fill?.text ?? ""} code={false} title="Mark fill" commit={(t) => commit(markSet(mark, "fill", t))} />
  {:else if view === "frame" && node}
    <select onchange={(e) => commit(e.currentTarget.value ? str(e.currentTarget.value) : null)}>
      <option value="" selected={node.kind === "none"}>none</option>
      {#each FRAMES as f}<option value={f} selected={node.kind === "str" && node.value === f}>{f}</option>{/each}
    </select>
  {:else if view === "anchor" && node?.kind === "str"}
    <select onchange={(e) => commit(str(e.currentTarget.value))}>
      {#each ANCHORS as a}<option value={a} selected={node.value === a}>{a}</option>{/each}
      {#if !ANCHORS.includes(node.value)}<option value={node.value} selected>{node.value}</option>{/if}
    </select>
  {:else if view === "number" && (node?.kind === "number" || node?.kind === "numeric")}
    <input class="number" type="number" step={NUMBER_KEYS[key]} value={num(node.value)} onchange={(e) => setNumber(e.currentTarget.value)} {onkeydown} />
    {#if node.kind === "numeric"}<span class="unit">{node.unit}</span>{/if}
  {:else if view === "bool" && node?.kind === "bool"}
    <input type="checkbox" checked={node.value} onchange={(e) => commit(String(e.currentTarget.checked))} />
  {:else}
    <input class="code" value={text} onchange={(e) => commit(e.currentTarget.value)} {onkeydown} spellcheck="false" />
  {/if}
  {#if view !== "color" && (raw || (node && view !== "code"))}
    <button class="icon toggle" class:on={raw} title={raw ? "Show fields" : "Edit as code"} onclick={() => (raw = !raw)}>&lt;/&gt;</button>
  {/if}
</div>

<style>
  .field {
    display: flex;
    gap: 4px;
    align-items: center;
    flex: 1;
    min-width: 0;
  }
  .code {
    flex: 1;
    min-width: 0;
  }
  .code.length {
    flex: 0 0 52px;
  }
  select {
    flex: 1;
    min-width: 0;
    font: inherit;
    color: inherit;
    background: var(--input-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 2px 2px;
    height: 22px;
  }
  .number {
    flex: 1;
    min-width: 0;
  }
  .unit {
    color: var(--muted);
  }
  .toggle {
    flex: none;
    margin-left: auto;
    font: 10px ui-monospace, "SF Mono", Menlo, monospace;
    color: var(--muted);
    padding: 0 4px;
    height: 22px;
  }
  .toggle.on {
    color: var(--accent);
    border-color: var(--accent);
  }
</style>
