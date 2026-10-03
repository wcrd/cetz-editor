<script lang="ts">
  // One option as a row, for one shape or several: its name (faded when no
  // shape gives it), its field, a remove button, and for strokes and marks a
  // disclosure with their less common keys as nested rows.
  import PropField from "./PropField.svelte";
  import PropRow from "./PropRow.svelte";
  import { partOf, setPart, type Change } from "./props";
  import { MARK_MORE, STROKE_MORE, type Opt } from "./schema";

  let {
    opt,
    texts,
    commit,
    onkeydown,
    depth = 0,
  }: {
    opt: Opt;
    /** Each selected shape's value source, "" where the option isn't given. */
    texts: string[];
    commit: (change: Change) => void;
    onkeydown: (e: KeyboardEvent) => void;
    depth?: number;
  } = $props();

  let open = $state(false);
  const given = $derived(texts.some((t) => t !== ""));

  /** Sub-options of a stroke or mark, when every shape's value can be read. */
  const more = $derived.by(() => {
    // Strokes inside marks don't need their own disclosure.
    const kind = opt.kind === "stroke" && depth === 0 ? "stroke" : opt.kind === "mark" ? "mark" : null;
    if (!kind || texts.some((t) => partOf(kind, t, "") === undefined)) return null;
    return {
      opts: kind === "stroke" ? STROKE_MORE : MARK_MORE,
      texts: (key: string) => texts.map((t) => partOf(kind, t, key) ?? ""),
      commit: (key: string) => (c: Change) => commit(setPart(kind, key, c)),
    };
  });
  const setCount = $derived(more ? more.opts.filter((o) => more.texts(o.key).some((t) => t !== "")).length : 0);
</script>

<div class="row" class:unset={!given} style:--depth={depth}>
  <span class="label" title={opt.help}>
    {#if more}
      <button class="disclose" class:open aria-expanded={open} title="{open ? 'Hide' : 'Show'} more {opt.key} options" onclick={() => (open = !open)}>▸</button>
    {/if}
    {opt.key}{#if more && setCount > 0 && !open}<span class="count">+{setCount}</span>{/if}
  </span>
  <PropField {opt} {texts} {commit} {onkeydown} />
  {#if given}
    <button class="icon" title="Remove {opt.key}" onclick={() => commit(null)}>×</button>
  {:else}
    <span></span>
  {/if}
</div>
{#if more && open}
  {#each more.opts as sub (sub.key)}
    <PropRow opt={sub} texts={more.texts(sub.key)} commit={more.commit(sub.key)} {onkeydown} depth={depth + 1} />
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
