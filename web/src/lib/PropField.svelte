<script lang="ts">
  // The value of one option, set or not: a widget for its kind (stroke,
  // mark, choice, number, ...) with CeTZ's default as the placeholder. A
  // value the widget can't read, or the </> button, shows the plain code.
  import ColorInput from "./ColorInput.svelte";
  import { num } from "./format";
  import { canonical, choiceLabel, DASHES, lengthText, MARKS, markEnd, markSet, parseExpr, parseMark, parseStroke, strokeGet, strokeSet } from "./props";
  import type { Opt } from "./schema";

  let {
    opt,
    text,
    commit,
    onkeydown,
  }: {
    opt: Opt;
    /** The value's source, or "" when the option isn't given. */
    text: string;
    commit: (text: string | null) => void;
    onkeydown: (e: KeyboardEvent) => void;
  } = $props();

  let raw = $state(false);
  const node = $derived(text ? parseExpr(text) : null);
  const stroke = $derived(opt.kind === "stroke" ? parseStroke(text) : null);
  const mark = $derived(opt.kind === "mark" ? parseMark(text) : null);
  const unset = $derived(text === "");

  /** Whether the widget for this kind can show the current value. */
  const readable = $derived.by(() => {
    if (unset) return true;
    switch (opt.kind) {
      case "stroke":
        return stroke !== null;
      case "mark":
        return mark !== null;
      case "choice":
        return opt.options.includes(canonical(text)) || canonical(text) === canonical(opt.default);
      case "number":
        return node?.kind === "number" || (node?.kind === "numeric" && (!opt.unit || node.unit === opt.unit));
      case "bool":
        return node?.kind === "bool";
      default:
        return true;
    }
  });
  const view = $derived(raw || !readable ? "code" : opt.kind);

  const str = (s: string) => JSON.stringify(s);

  function setNumber(value: string) {
    if (value.trim() === "") return commit(null);
    const n = Number(value);
    if (!Number.isFinite(n)) return;
    const unit = node?.kind === "numeric" ? node.unit : (opt.kind === "number" && opt.unit) || "";
    commit(`${num(n)}${unit}`);
  }
</script>

<div class="field" class:unset>
  {#if view === "color"}
    <ColorInput {text} {commit} {onkeydown} title={opt.key} placeholder={opt.default} />
  {:else if view === "stroke" && stroke}
    {@const paint = strokeGet(stroke, "paint")}
    {@const dash = strokeGet(stroke, "dash")}
    {@const current = dash?.node.kind === "str" ? dash.node.value : dash ? dash.text : ""}
    <ColorInput text={stroke.none ? "none" : (paint?.text ?? "")} code={false} title="Stroke color" commit={(t) => commit(strokeSet(stroke, "paint", t))} />
    <input
      class="code length"
      title="Thickness"
      placeholder={stroke.none ? "none" : "1pt"}
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
  {:else if view === "choice" && opt.kind === "choice"}
    {@const current = unset ? "" : canonical(text)}
    <select onchange={(e) => commit(e.currentTarget.value || null)}>
      <option value="" selected={current === ""}>{choiceLabel(opt.default)}</option>
      {#each opt.options.filter((o) => o !== canonical(opt.default)) as o}<option value={o} selected={current === o}>{choiceLabel(o)}</option>{/each}
    </select>
  {:else if view === "number" && opt.kind === "number"}
    <input
      class="number"
      type="number"
      step={opt.step}
      placeholder={opt.default.replace(opt.unit ?? "", "")}
      value={node?.kind === "number" || node?.kind === "numeric" ? num(node.value) : ""}
      onchange={(e) => setNumber(e.currentTarget.value)}
      {onkeydown}
    />
    {#if opt.unit || node?.kind === "numeric"}<span class="unit">{node?.kind === "numeric" ? node.unit : opt.unit}</span>{/if}
  {:else if view === "bool"}
    <input type="checkbox" checked={node?.kind === "bool" ? node.value : opt.default === "true"} onchange={(e) => commit(String(e.currentTarget.checked))} />
  {:else if view === "length"}
    <input class="code" value={text} placeholder={opt.default} onchange={(e) => commit(lengthText(e.currentTarget.value))} {onkeydown} spellcheck="false" />
  {:else}
    <input class="code" value={text} placeholder={opt.default} onchange={(e) => commit(e.currentTarget.value.trim() || null)} {onkeydown} spellcheck="false" />
  {/if}
  {#if !unset && opt.kind !== "code" && opt.kind !== "length" && opt.kind !== "color" && (raw || readable)}
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
  .unset select {
    color: var(--muted);
  }
  .number {
    flex: 1;
    min-width: 0;
  }
  .unit {
    color: var(--muted);
  }
  .field .toggle {
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
