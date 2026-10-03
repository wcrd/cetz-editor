<script lang="ts">
  // The toolbar's grid step: type any positive size (`0.3`) or fraction
  // (`1/3`), or pick a preset. Shared by every tab and remembered in this
  // browser.
  import { onMount } from "svelte";
  import type { Editor } from "./editor.svelte";
  import { formatStep, GRID_STEP_MAX, GRID_STEP_MIN, parseStep } from "./pixels";

  let { editor }: { editor: Editor } = $props();

  const PRESETS = ["0.1", "0.25", "0.5", "1", "1/3", "1/4"];
  const KEY = "cetz-editor:grid-step";

  // Restore the last step once, when the toolbar first appears.
  onMount(() => {
    try {
      const saved = parseStep(localStorage.getItem(KEY) ?? "");
      if (saved !== undefined) editor.gridStep = saved;
    } catch {
      // Storage unavailable: keep the default.
    }
  });

  let input: HTMLInputElement;

  function commit() {
    const value = parseStep(input.value);
    if (value === undefined) {
      if (input.value.trim() !== "") editor.flash(`Grid step must be a number from ${GRID_STEP_MIN} to ${GRID_STEP_MAX}, or a fraction like 1/3`);
      input.value = formatStep(editor.gridStep);
      return;
    }
    editor.gridStep = value;
    input.value = formatStep(value);
    try {
      localStorage.setItem(KEY, input.value);
    } catch {
      // Not remembered; that's fine.
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter") input.blur();
    if (e.key === "Escape") {
      input.value = formatStep(editor.gridStep);
      input.blur();
    }
  }
</script>

<input
  bind:this={input}
  class="grid-step"
  list="grid-step-presets"
  value={formatStep(editor.gridStep)}
  onchange={commit}
  {onkeydown}
  onfocus={() => input.select()}
  spellcheck="false"
  inputmode="decimal"
  aria-label="Grid step"
  title="Grid step in canvas units: any number from {GRID_STEP_MIN} to {GRID_STEP_MAX}, or a fraction like 1/3"
/>
<datalist id="grid-step-presets">
  {#each PRESETS as preset}<option value={preset}></option>{/each}
</datalist>

<style>
  .grid-step {
    width: 64px;
    height: 28px;
    box-sizing: border-box;
    padding: 0 6px;
    font: inherit;
    font-variant-numeric: tabular-nums;
    color: inherit;
    background: var(--input-bg);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .grid-step:focus {
    outline: 2px solid color-mix(in srgb, var(--accent) 40%, transparent);
    border-color: var(--accent);
  }
</style>
