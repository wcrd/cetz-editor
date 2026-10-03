<script lang="ts">
  // One option as a row: its name (faded when not given), its field, a
  // remove button, and for strokes and marks a disclosure with their less
  // common keys as nested rows.
  import PropField from "./PropField.svelte";
  import PropRow from "./PropRow.svelte";
  import { markSet, parseMark, parseStroke, strokeSet } from "./props";
  import { MARK_MORE, STROKE_MORE, type Opt } from "./schema";

  let {
    opt,
    text,
    commit,
    onkeydown,
    depth = 0,
  }: {
    opt: Opt;
    text: string;
    commit: (text: string | null) => void;
    onkeydown: (e: KeyboardEvent) => void;
    depth?: number;
  } = $props();

  let open = $state(false);

  /** Sub-options, read from and written back into this value. */
  const more = $derived.by(() => {
    if (opt.kind === "stroke") {
      const s = parseStroke(text);
      // Strokes inside marks don't need their own disclosure.
      if (s && depth === 0) return { opts: STROKE_MORE, get: (k: string) => s.entries.find((e) => e.key === k)?.text ?? "", set: (k: string, t: string | null) => strokeSet(s, k, t) };
    }
    if (opt.kind === "mark") {
      const m = parseMark(text);
      if (m) return { opts: MARK_MORE, get: (k: string) => m.find((e) => e.key === k)?.text ?? "", set: (k: string, t: string | null) => markSet(m, k, t) };
    }
    return null;
  });
  const setCount = $derived(more ? more.opts.filter((o) => more.get(o.key) !== "").length : 0);
</script>

<div class="row" class:unset={text === ""} style:--depth={depth}>
  <span class="label" title={opt.help}>
    {#if more}
      <button class="disclose" class:open aria-expanded={open} title="{open ? 'Hide' : 'Show'} more {opt.key} options" onclick={() => (open = !open)}>▸</button>
    {/if}
    {opt.key}{#if more && setCount > 0 && !open}<span class="count">+{setCount}</span>{/if}
  </span>
  <PropField {opt} {text} {commit} {onkeydown} />
  {#if text !== ""}
    <button class="icon" title="Remove {opt.key}" onclick={() => commit(null)}>×</button>
  {:else}
    <span></span>
  {/if}
</div>
{#if more && open}
  {#each more.opts as sub (sub.key)}
    <PropRow opt={sub} text={more.get(sub.key)} commit={(t) => commit(more.set(sub.key, t))} {onkeydown} depth={depth + 1} />
  {/each}
{/if}

<style>
  .row {
    display: grid;
    grid-template-columns: var(--label-width, 92px) minmax(0, 1fr) 22px;
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
  }
  .label {
    display: flex;
    align-items: center;
    gap: 2px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding-left: calc(var(--depth) * 12px);
  }
  .row:not(.unset) .label {
    color: inherit;
    font-weight: 500;
  }
  .count {
    margin-left: 4px;
    font-size: 10.5px;
    color: var(--muted);
  }
  /* Beats the inspector's shared button style. */
  .label .disclose {
    border: none;
    background: none;
    padding: 0 2px;
    margin-left: -4px;
    color: var(--muted);
    font-size: 10px;
    line-height: 1;
    transition: transform 0.1s;
  }
  .label .disclose.open {
    transform: rotate(90deg);
  }
  .row .icon {
    padding: 0;
    width: 22px;
    height: 22px;
    line-height: 20px;
  }
</style>
