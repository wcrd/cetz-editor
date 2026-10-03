<script lang="ts">
  // The value of one option, set or not, for one shape or several: a widget
  // for its kind (stroke, mark, choice, number, ...) with CeTZ's default as
  // the placeholder, or "mixed" where the shapes differ. A value the widget
  // can't read, or the </> button, shows the plain code.
  import ColorInput from "./ColorInput.svelte";
  import { num } from "./format";
  import { canonical, choiceLabel, common, DASHES, lengthText, MARKS, parseExpr, parseMark, parseStroke, partOf, setPart, type Change } from "./props";
  import type { Opt } from "./schema";

  let {
    opt,
    texts,
    commit,
    onkeydown,
  }: {
    opt: Opt;
    /** Each selected shape's value source, "" where the option isn't given. */
    texts: string[];
    commit: (change: Change) => void;
    onkeydown: (e: KeyboardEvent) => void;
  } = $props();

  /** The value of the default option in a select. */
  const DEFAULT = "\u0000default";

  let raw = $state(false);
  const value = $derived(common(texts));
  const text = $derived(value.text);
  const mixed = $derived(value.mixed);
  const node = $derived(text ? parseExpr(text) : null);
  const unset = $derived(text === "" && !mixed);

  /** A stroke's or mark's key, shared across the shapes. */
  const part = (key: string) => common(texts.map((t) => partOf(opt.kind as "stroke" | "mark", t, key) ?? ""));

  /** Whether the widget for this kind can show every shape's value. */
  const readable = $derived(
    texts.every((t) => {
      if (t === "") return true;
      const n = parseExpr(t);
      switch (opt.kind) {
        case "stroke":
          return parseStroke(t) !== null;
        case "mark":
          return parseMark(t) !== null;
        case "choice":
          return opt.options.includes(canonical(t)) || canonical(t) === canonical(opt.default);
        case "number":
          return n?.kind === "number" || (n?.kind === "numeric" && (!opt.unit || n.unit === opt.unit));
        case "bool":
          return n?.kind === "bool";
        default:
          return true;
      }
    }),
  );
  const view = $derived(raw || !readable ? "code" : opt.kind);
  const placeholder = $derived(mixed ? "mixed" : opt.default);

  const str = (s: string) => JSON.stringify(s);
  const fromSelect = (v: string) => (v === DEFAULT || v === "" ? null : v);

  function setNumber(input: string) {
    if (input.trim() === "") return commit(null);
    const n = Number(input);
    if (!Number.isFinite(n)) return;
    const unit = node?.kind === "numeric" ? node.unit : (opt.kind === "number" && opt.unit) || "";
    commit(`${num(n)}${unit}`);
  }

  function indeterminate(el: HTMLInputElement, on: boolean) {
    el.indeterminate = on;
    return { update: (v: boolean) => (el.indeterminate = v) };
  }
</script>

<div class="field" class:unset={unset || mixed}>
  {#if view === "color"}
    <ColorInput {text} {mixed} {commit} {onkeydown} title={opt.key} {placeholder} />
  {:else if view === "stroke"}
    {@const paint = part("paint")}
    {@const thickness = part("thickness")}
    {@const dash = part("dash")}
    {@const current = dash.mixed ? null : dash.text ? choiceLabel(dash.text) : ""}
    <ColorInput text={text.trim() === "none" ? "none" : paint.text} mixed={paint.mixed} code={false} title="Stroke color" commit={(t) => commit(setPart("stroke", "paint", t))} />
    <input
      class="code length"
      title="Thickness"
      placeholder={thickness.mixed ? "mixed" : text.trim() === "none" ? "none" : "1pt"}
      value={thickness.text}
      onchange={(e) => commit(setPart("stroke", "thickness", lengthText(e.currentTarget.value)))}
      {onkeydown}
      spellcheck="false"
    />
    <select title="Dash pattern" onchange={(e) => commit(setPart("stroke", "dash", fromSelect(e.currentTarget.value) && str(e.currentTarget.value)))}>
      {#if current === null}<option value="" selected disabled>mixed</option>{/if}
      <option value={DEFAULT} selected={current === ""}>solid</option>
      {#each DASHES.slice(1) as d}<option value={d} selected={current === d}>{d}</option>{/each}
      {#if current && !DASHES.includes(current)}<option value={current} selected>{current}</option>{/if}
    </select>
  {:else if view === "mark"}
    {@const fill = part("fill")}
    {#each ["start", "end"] as const as end}
      {@const p = part(end)}
      {@const current = p.mixed ? null : p.text ? choiceLabel(p.text) : ""}
      <select title="{end === 'start' ? 'Start' : 'End'} mark" onchange={(e) => commit(setPart("mark", end, fromSelect(e.currentTarget.value) && str(e.currentTarget.value)))}>
        {#if current === null}<option value="" selected disabled>mixed</option>{/if}
        <option value={DEFAULT} selected={current === ""}>none</option>
        {#each MARKS as [sym, name]}<option value={sym} selected={current === sym}>{name}</option>{/each}
        {#if current && !MARKS.some(([s]) => s === current)}<option value={current} selected>{current}</option>{/if}
      </select>
    {/each}
    <ColorInput text={fill.text} mixed={fill.mixed} code={false} title="Mark fill" commit={(t) => commit(setPart("mark", "fill", t))} />
  {:else if view === "choice" && opt.kind === "choice"}
    {@const current = mixed ? null : unset ? "" : canonical(text)}
    <select onchange={(e) => commit(fromSelect(e.currentTarget.value))}>
      {#if current === null}<option value="" selected disabled>mixed</option>{/if}
      <option value={DEFAULT} selected={current === ""}>{choiceLabel(opt.default)}</option>
      {#each opt.options.filter((o) => o !== canonical(opt.default)) as o}<option value={o} selected={current === o}>{choiceLabel(o)}</option>{/each}
    </select>
  {:else if view === "number" && opt.kind === "number"}
    <input
      class="number"
      type="number"
      step={opt.step}
      placeholder={mixed ? "mixed" : opt.default.replace(opt.unit ?? "", "")}
      value={node?.kind === "number" || node?.kind === "numeric" ? num(node.value) : ""}
      onchange={(e) => setNumber(e.currentTarget.value)}
      {onkeydown}
    />
    {#if opt.unit || node?.kind === "numeric"}<span class="unit">{node?.kind === "numeric" ? node.unit : opt.unit}</span>{/if}
  {:else if view === "bool"}
    <input
      type="checkbox"
      title={mixed ? "mixed" : undefined}
      checked={node?.kind === "bool" ? node.value : !mixed && opt.default === "true"}
      use:indeterminate={mixed}
      onchange={(e) => commit(String(e.currentTarget.checked))}
    />
  {:else if view === "length"}
    <input class="code" value={text} {placeholder} onchange={(e) => commit(lengthText(e.currentTarget.value))} {onkeydown} spellcheck="false" />
  {:else}
    <input class="code" value={text} {placeholder} onchange={(e) => commit(e.currentTarget.value.trim() || null)} {onkeydown} spellcheck="false" />
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
