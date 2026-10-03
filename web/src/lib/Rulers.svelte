<script module lang="ts">
  /** Thickness of each ruler, in screen pixels. */
  export const RULER = 20;
</script>

<script lang="ts">
  // Rulers along the canvas's top and left edges, in the active canvas's CeTZ
  // units (y up, origin at (0, 0)), marking the cursor and the selection's
  // extent. Drawn over the viewport in screen pixels.
  import type { Editor } from "./editor.svelte";
  import { num } from "./format";
  import { crisp, visibleStep } from "./pixels";

  type Box = { x0: number; y0: number; x1: number; y1: number };
  let {
    editor,
    width,
    height,
    cursor,
    band,
  }: {
    editor: Editor;
    width: number;
    height: number;
    /** Pointer position in screen pixels. */
    cursor?: [number, number];
    /** The selection's bounds in page points. */
    band?: Box;
  } = $props();

  const frame = $derived(editor.frameFor(editor.activeCanvas));
  /** Screen pixels per CeTZ unit. */
  const scale = $derived(frame.length * editor.zoom);

  const screenX = (x: number) => editor.pan[0] + (frame.origin.x + x * frame.length) * editor.zoom;
  const screenY = (y: number) => editor.pan[1] + (frame.origin.y - y * frame.length) * editor.zoom;
  const unitX = (sx: number) => ((sx - editor.pan[0]) / editor.zoom - frame.origin.x) / frame.length;
  const unitY = (sy: number) => (frame.origin.y - (sy - editor.pan[1]) / editor.zoom) / frame.length;

  /** Minor ticks on the grid step (thinned like the grid); labels on round numbers that land on one. */
  const steps = $derived.by(() => {
    const minor = visibleStep(editor.gridStep, scale);
    for (let exp = -3; exp <= 9; exp++) {
      for (const m of [1, 2, 2.5, 5]) {
        const major = m * 10 ** exp;
        const ratio = major / minor;
        if (major * scale >= 56 && ratio >= 1 && Math.abs(ratio - Math.round(ratio)) < 1e-6) return { minor, major };
      }
    }
    return { minor, major: minor };
  });

  type Tick = { at: number; label?: string };
  function ticks(from: number, to: number, toScreen: (v: number) => number): Tick[] {
    const { minor, major } = steps;
    const [lo, hi] = from < to ? [from, to] : [to, from];
    const out: Tick[] = [];
    for (let i = Math.ceil(lo / minor); i <= Math.floor(hi / minor); i++) {
      const v = i * minor;
      const r = v / major;
      out.push({ at: crisp(toScreen(v)), label: Math.abs(r - Math.round(r)) < 1e-6 ? num(v) : undefined });
    }
    return out;
  }

  const xTicks = $derived(ticks(unitX(RULER), unitX(width), screenX));
  const yTicks = $derived(ticks(unitY(height), unitY(RULER), screenY));
  const tickPath = (ts: Tick[], horizontal: boolean) =>
    ts.map((t) => (horizontal ? `M${t.at},${RULER - (t.label ? 8 : 4)}V${RULER}` : `M${RULER - (t.label ? 8 : 4)},${t.at}H${RULER}`)).join("");

  const bandScreen = $derived(
    band && {
      x0: editor.pan[0] + band.x0 * editor.zoom,
      x1: editor.pan[0] + band.x1 * editor.zoom,
      y0: editor.pan[1] + band.y0 * editor.zoom,
      y1: editor.pan[1] + band.y1 * editor.zoom,
    },
  );

  const readout = $derived(cursor && { x: num(Math.round(unitX(cursor[0]) * 100) / 100), y: num(Math.round(unitY(cursor[1]) * 100) / 100) });

  // Clicks on a ruler shouldn't start a selection or a shape underneath it.
  const stop = (e: PointerEvent) => e.stopPropagation();
</script>

<svg class="rulers" {width} {height} aria-hidden="true" role="presentation" onpointerdown={stop}>
  <rect class="bg" x="0" y="0" width={width} height={RULER} />
  <rect class="bg" x="0" y="0" width={RULER} height={height} />

  {#if bandScreen}
    <rect class="band" x={bandScreen.x0} y="0" width={bandScreen.x1 - bandScreen.x0} height={RULER} />
    <rect class="band" x="0" y={bandScreen.y0} width={RULER} height={bandScreen.y1 - bandScreen.y0} />
  {/if}

  <path class="tick" d={tickPath(xTicks, true) + tickPath(yTicks, false)} />
  {#each xTicks as t}
    {#if t.label !== undefined}<text x={t.at + 3} y="9">{t.label}</text>{/if}
  {/each}
  {#each yTicks as t}
    {#if t.label !== undefined}<text transform="translate(9 {t.at - 3}) rotate(-90)">{t.label}</text>{/if}
  {/each}

  {#if cursor}
    <path class="cursor" d="M{crisp(cursor[0])},0V{RULER}M0,{crisp(cursor[1])}H{RULER}" />
  {/if}

  <rect class="bg corner" x="0" y="0" width={RULER} height={RULER} />
  <path class="edge" d="M{RULER - 0.5},{RULER - 0.5}H{width}M{RULER - 0.5},{RULER - 0.5}V{height}" />
</svg>

{#if readout}
  <div class="readout" style:left="{RULER + 8}px">x {readout.x} · y {readout.y}</div>
{/if}

<style>
  .rulers {
    position: absolute;
    inset: 0;
    pointer-events: none;
    font: 9.5px ui-monospace, "SF Mono", Menlo, monospace;
  }
  .bg {
    fill: var(--panel);
    pointer-events: all;
    cursor: default;
  }
  .band {
    fill: color-mix(in srgb, var(--accent) 16%, transparent);
  }
  .tick,
  .edge {
    fill: none;
    stroke: var(--border);
    stroke-width: 1;
  }
  .tick {
    stroke: var(--muted);
    opacity: 0.7;
  }
  text {
    fill: var(--muted);
  }
  .cursor {
    stroke: var(--accent);
    stroke-width: 1;
  }
  .readout {
    position: absolute;
    bottom: 8px;
    padding: 2px 7px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--panel) 88%, transparent);
    border: 1px solid var(--border);
    color: var(--muted);
    font: 11px ui-monospace, "SF Mono", Menlo, monospace;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
    white-space: nowrap;
  }
</style>
