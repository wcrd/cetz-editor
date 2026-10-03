<script lang="ts">
  // A swatch for a color expression, optionally with its code. The swatch shows
  // what the expression evaluates to when we can tell (`red`, `luma(90%)`,
  // `orange.lighten(85%)`); picking a color writes `rgb("#...")` or a name.
  import { colorHex, colorText, parseExpr } from "./props";

  let {
    text,
    commit,
    code = true,
    title = "Color",
    onkeydown,
  }: {
    /** The color expression, or "" when unset. */
    text: string;
    commit: (text: string | null) => void;
    /** Also show the expression as an editable code box. */
    code?: boolean;
    title?: string;
    onkeydown?: (e: KeyboardEvent) => void;
  } = $props();

  const node = $derived(text ? parseExpr(text) : null);
  const hex = $derived(colorHex(node));
  const state = $derived(!text || node?.kind === "auto" ? "unset" : node?.kind === "none" ? "none" : hex ? "known" : "unknown");
</script>

<span class="color" class:wide={code}>
  <label class="swatch {state}" style:--swatch={hex} title={text ? `${title}: ${text}` : `${title}: not set`}>
    <input type="color" value={hex ?? "#000000"} onchange={(e) => commit(colorText(e.currentTarget.value))} />
  </label>
  {#if code}
    <input
      class="code"
      value={text}
      placeholder="none"
      onchange={(e) => commit(e.currentTarget.value.trim() || null)}
      {onkeydown}
      spellcheck="false"
    />
  {/if}
</span>

<style>
  .color {
    display: inline-flex;
    gap: 4px;
    align-items: center;
    min-width: 0;
  }
  .color.wide {
    flex: 1;
  }
  .color.wide .code {
    flex: 1;
    min-width: 0;
  }
  .swatch {
    position: relative;
    flex: none;
    width: 22px;
    height: 22px;
    border-radius: 4px;
    border: 1px solid var(--border);
    box-sizing: border-box;
    cursor: pointer;
    background: var(--swatch);
    overflow: hidden;
  }
  .swatch input {
    position: absolute;
    inset: 0;
    opacity: 0;
    width: 100%;
    height: 100%;
    cursor: pointer;
    padding: 0;
    border: none;
  }
  /* Not set / none: a slash. Unknown (a gradient, a variable): a checkerboard. */
  .swatch.unset,
  .swatch.none {
    background: linear-gradient(to top right, transparent calc(50% - 1px), var(--muted) 50%, transparent calc(50% + 1px)), var(--input-bg);
  }
  .swatch.unknown {
    background: repeating-conic-gradient(var(--border) 0 25%, var(--input-bg) 0 50%) 0 0 / 8px 8px;
  }
  .swatch:focus-within {
    outline: 2px solid color-mix(in srgb, var(--accent) 40%, transparent);
  }
</style>
